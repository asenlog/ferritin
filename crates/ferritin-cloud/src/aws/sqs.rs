//! SQS results-queue listener — the inbound leg of the cloud round
//! trip. Mirrors the flow of the Orthanc `aws-sqs` plugin: long-poll
//! the queue, treat every message as an S3 Event Notification, fetch
//! the referenced object, hand it to the pipeline, and delete the
//! message only when handling succeeded. Failures stay on the queue
//! and become visible again after the visibility timeout; poison
//! messages land in the DLQ via the queue's redrive policy (which is
//! configured on the SQS side, not here).

use anyhow::Context;
use serde::Deserialize;
use std::sync::Arc;

/// Long-poll wait time for each receive round, in seconds.
const WAIT_TIME_SECONDS: i32 = 20;
/// Per-receive batch size (SQS maximum).
const MAX_MESSAGES: i32 = 10;

/// A result object fetched from S3 as referenced by a queue message.
#[derive(Debug)]
pub struct FetchedResult {
    pub bucket: String,
    pub key: String,
    pub bytes: Vec<u8>,
}

/// Polls one SQS queue for S3-event messages and fetches the objects
/// they reference. Owns a private tokio runtime, same sync-bridge
/// pattern as the other adapters.
pub struct SqsResultListener {
    sqs: aws_sdk_sqs::Client,
    s3: aws_sdk_s3::Client,
    queue_url: String,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl SqsResultListener {
    /// Build from the default AWS config chain — the production path.
    pub fn connect(queue_url: &str) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for SQS listener")?;
        let (sqs, s3) = runtime.block_on(async {
            let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            (
                aws_sdk_sqs::Client::new(&config),
                aws_sdk_s3::Client::new(&config),
            )
        });
        Ok(Self::from_clients(sqs, s3, queue_url, Arc::new(runtime)))
    }

    /// Build on preconfigured clients — tests (LocalStack) and
    /// deployments that need custom endpoints or credentials.
    pub fn from_clients(
        sqs: aws_sdk_sqs::Client,
        s3: aws_sdk_s3::Client,
        queue_url: &str,
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        Self {
            sqs,
            s3,
            queue_url: queue_url.to_string(),
            runtime,
        }
    }

    /// Poll and handle messages until an unrecoverable error occurs.
    /// Per-message failures are logged and left on the queue.
    pub fn run(&self, handler: impl Fn(FetchedResult) -> anyhow::Result<()>) -> anyhow::Result<()> {
        loop {
            self.poll_once(&handler)?;
        }
    }

    /// One long-poll round. For every S3-event message: fetch each
    /// referenced object, hand it to `handler`, and delete the message
    /// only when every record in it was handled. Returns how many
    /// messages were successfully handled and deleted.
    pub fn poll_once(
        &self,
        handler: impl Fn(FetchedResult) -> anyhow::Result<()>,
    ) -> anyhow::Result<usize> {
        self.runtime.block_on(async {
            let received = self
                .sqs
                .receive_message()
                .queue_url(&self.queue_url)
                .wait_time_seconds(WAIT_TIME_SECONDS)
                .max_number_of_messages(MAX_MESSAGES)
                .send()
                .await
                .context("failed to receive from SQS")?;

            let mut handled = 0;
            for message in received.messages() {
                let receipt = message.receipt_handle().unwrap_or_default().to_string();
                match self
                    .handle_message(message.body().unwrap_or_default(), &handler)
                    .await
                {
                    Ok(()) => {
                        self.sqs
                            .delete_message()
                            .queue_url(&self.queue_url)
                            .receipt_handle(&receipt)
                            .send()
                            .await
                            .context("failed to delete handled message")?;
                        handled += 1;
                    }
                    Err(e) => {
                        // not deleted: returns to the queue after the
                        // visibility timeout, DLQ catches poisons
                        tracing::warn!("leaving message on queue after failure: {e:#}");
                    }
                }
            }
            Ok(handled)
        })
    }

    async fn handle_message(
        &self,
        body: &str,
        handler: &impl Fn(FetchedResult) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let event = parse_s3_event(body)?;
        for record in event.records {
            let key = url_decode(&record.s3.object.key)
                .with_context(|| format!("bad object key encoding: {:?}", record.s3.object.key))?;
            let object = self
                .s3
                .get_object()
                .bucket(&record.s3.bucket.name)
                .key(&key)
                .send()
                .await
                .with_context(|| format!("failed to fetch s3://{}/{key}", record.s3.bucket.name))?;
            let bytes = object.body.collect().await?.into_bytes().to_vec();
            handler(FetchedResult {
                bucket: record.s3.bucket.name,
                key,
                bytes,
            })?;
        }
        Ok(())
    }
}

/// The S3 Event Notification shape, as emitted by a bucket's queue
/// configuration (and by SNS-fanned-out events after unwrapping).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct S3Event {
    #[serde(default)]
    records: Vec<S3EventRecord>,
}

#[derive(Debug, Deserialize)]
struct S3EventRecord {
    s3: S3Entity,
}

#[derive(Debug, Deserialize)]
struct S3Entity {
    bucket: S3Bucket,
    object: S3Object,
}

#[derive(Debug, Deserialize)]
struct S3Bucket {
    name: String,
}

#[derive(Debug, Deserialize)]
struct S3Object {
    key: String,
}

/// Parse a queue message body as an S3 Event Notification.
fn parse_s3_event(body: &str) -> anyhow::Result<S3Event> {
    serde_json::from_str(body).context("message is not an S3 event notification")
}

/// S3 event keys are URL-encoded (`+` for space, `%XX` for the rest).
fn url_decode(encoded: &str) -> anyhow::Result<String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|pair| std::str::from_utf8(pair).ok())
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .with_context(|| format!("truncated escape in {encoded:?}"))?;
                out.push(hex);
                i += 3;
            }
            plain => {
                out.push(plain);
                i += 1;
            }
        }
    }
    String::from_utf8(out).context("decoded key is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_s3_event_notification() {
        let body = r#"{
            "Records": [{
                "eventVersion": "2.1",
                "eventSource": "aws:s3",
                "eventName": "ObjectCreated:Put",
                "s3": {
                    "bucket": { "name": "ferritin-results", "arn": "arn:aws:s3:::ferritin-results" },
                    "object": { "key": "results/1.2.3/result.dcm", "size": 47821 }
                }
            }]
        }"#;

        let event = parse_s3_event(body).unwrap();

        assert_eq!(event.records.len(), 1);
        assert_eq!(event.records[0].s3.bucket.name, "ferritin-results");
        assert_eq!(event.records[0].s3.object.key, "results/1.2.3/result.dcm");
    }

    #[test]
    fn rejects_non_event_bodies() {
        assert!(parse_s3_event("not json").is_err());
        assert!(parse_s3_event(r#"{"Records": "nope"}"#).is_err());
    }

    #[test]
    fn url_decodes_s3_keys() {
        assert_eq!(url_decode("plain/key.dcm").unwrap(), "plain/key.dcm");
        assert_eq!(url_decode("a+b.dcm").unwrap(), "a b.dcm");
        assert_eq!(
            url_decode("studies/%2Froot.dcm").unwrap(),
            "studies//root.dcm"
        );
        assert_eq!(url_decode("100%25.dcm").unwrap(), "100%.dcm");
    }

    #[test]
    fn rejects_truncated_escapes() {
        assert!(url_decode("bad%").is_err());
        assert!(url_decode("bad%2").is_err());
        assert!(url_decode("bad%zz.dcm").is_err());
    }
}
