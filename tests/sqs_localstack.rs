//! SQS listener integration test against a throwaway LocalStack
//! container — the full flow: S3 event message lands on the queue,
//! the listener fetches the referenced object and deletes the message
//! on success; failures stay on the queue.

use aws_config::BehaviorVersion;
use ferritin::infra::cloud::aws::sqs::{FetchedResult, SqsResultListener};
use std::sync::{Arc, Mutex};
use testcontainers::runners::SyncRunner;
use testcontainers_modules::localstack::LocalStack;

const BUCKET: &str = "ferritin-results";
const QUEUE: &str = "results-queue";

fn s3_event_body(bucket: &str, key: &str) -> String {
    format!(
        r#"{{"Records": [{{
            "eventSource": "aws:s3",
            "eventName": "ObjectCreated:Put",
            "s3": {{
                "bucket": {{ "name": "{bucket}" }},
                "object": {{ "key": "{key}", "size": 42 }}
            }}
        }}]}}"#
    )
}

struct Rig {
    _container: testcontainers::Container<LocalStack>,
    listener: SqsResultListener,
    sqs: aws_sdk_sqs::Client,
    s3: aws_sdk_s3::Client,
    queue_url: String,
    runtime: Arc<tokio::runtime::Runtime>,
}

fn rig() -> Rig {
    let container = LocalStack::default()
        .start()
        .expect("failed to start localstack container — is Docker running?");
    let port = container.get_host_port_ipv4(4566).unwrap();
    let endpoint = format!("http://127.0.0.1:{port}");

    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
    );
    let (sqs, s3) = runtime.block_on(async {
        let credentials =
            aws_sdk_s3::config::Credentials::new("test", "test", None, None, "localstack");
        let shared = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(credentials)
            .endpoint_url(&endpoint)
            .load()
            .await;
        let s3_config = aws_sdk_s3::config::Builder::from(&shared)
            .force_path_style(true)
            .build();
        (
            aws_sdk_sqs::Client::new(&shared),
            aws_sdk_s3::Client::from_conf(s3_config),
        )
    });

    runtime.block_on(async {
        s3.create_bucket().bucket(BUCKET).send().await.unwrap();
        // visibility timeout 0: a failed message is receivable again
        // immediately, which keeps the failure-path assertion fast
        sqs.create_queue()
            .queue_name(QUEUE)
            .attributes(
                aws_sdk_sqs::types::QueueAttributeName::VisibilityTimeout,
                "0",
            )
            .send()
            .await
            .unwrap();
    });
    // localstack's returned QueueUrl points at its internal endpoint;
    // build the URL against the endpoint we can actually reach
    let queue_url = format!("{endpoint}/000000000000/{QUEUE}");

    let listener =
        SqsResultListener::from_clients(sqs.clone(), s3.clone(), &queue_url, runtime.clone());
    Rig {
        _container: container,
        listener,
        sqs,
        s3,
        queue_url,
        runtime,
    }
}

#[test]
fn fetched_result_is_handled_and_message_deleted() {
    let rig = rig();
    let key = "results/1.2.3/result.dcm";
    let payload = b"processed result bytes";

    rig.runtime.block_on(async {
        rig.s3
            .put_object()
            .bucket(BUCKET)
            .key(key)
            .body(aws_sdk_s3::primitives::ByteStream::from(payload.to_vec()))
            .send()
            .await
            .unwrap();
        rig.sqs
            .send_message()
            .queue_url(&rig.queue_url)
            .message_body(s3_event_body(BUCKET, key))
            .send()
            .await
            .unwrap();
    });

    let handled: Mutex<Vec<(String, String, Vec<u8>)>> = Mutex::new(Vec::new());
    let count = rig
        .listener
        .poll_once(|result: FetchedResult| {
            handled
                .lock()
                .unwrap()
                .push((result.bucket, result.key, result.bytes));
            Ok(())
        })
        .unwrap();

    assert_eq!(count, 1);
    let handled = handled.lock().unwrap();
    assert_eq!(handled.len(), 1);
    assert_eq!(handled[0].0, BUCKET);
    assert_eq!(handled[0].1, key);
    assert_eq!(handled[0].2, payload);

    // the handled message was deleted: a short poll comes back empty
    let received = rig
        .runtime
        .block_on(
            rig.sqs
                .receive_message()
                .queue_url(&rig.queue_url)
                .wait_time_seconds(0)
                .send(),
        )
        .unwrap();
    assert!(received.messages().is_empty());
}

#[test]
fn failed_handling_leaves_message_on_the_queue() {
    let rig = rig();
    let key = "results/9.9.9/missing.dcm";

    rig.runtime.block_on(async {
        // note: no object uploaded — the fetch must fail
        rig.sqs
            .send_message()
            .queue_url(&rig.queue_url)
            .message_body(s3_event_body(BUCKET, key))
            .send()
            .await
            .unwrap();
    });

    let count = rig.listener.poll_once(|_| Ok(())).unwrap();
    assert_eq!(count, 0, "nothing may be deleted on failure");

    // visibility timeout is 0: the poison message is back at once
    let received = rig
        .runtime
        .block_on(
            rig.sqs
                .receive_message()
                .queue_url(&rig.queue_url)
                .wait_time_seconds(0)
                .send(),
        )
        .unwrap();
    assert_eq!(received.messages().len(), 1);
}
