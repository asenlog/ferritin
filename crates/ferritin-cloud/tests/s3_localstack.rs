//! S3 adapter integration test against a throwaway LocalStack
//! container. Requires a running Docker daemon — same convention as
//! the core Postgres tests.

use aws_config::BehaviorVersion;
use ferritin_cloud::aws::s3::S3ObjectStore;
use ferritin_core::ports::ObjectStore;
use std::sync::Arc;
use testcontainers::runners::SyncRunner;
use testcontainers_modules::localstack::LocalStack;

const BUCKET: &str = "ferritin-test";

#[test]
fn s3_put_round_trip() {
    let container = LocalStack::default()
        .start()
        .expect("failed to start localstack container — is Docker running?");
    let port = container.get_host_port_ipv4(4566).unwrap();

    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
    );
    let client = runtime.block_on(async {
        let credentials =
            aws_sdk_s3::config::Credentials::new("test", "test", None, None, "localstack");
        let shared = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(credentials)
            .endpoint_url(format!("http://127.0.0.1:{port}"))
            .load()
            .await;
        // localstack addressing is path-style, not virtual-host
        let s3_config = aws_sdk_s3::config::Builder::from(&shared)
            .force_path_style(true)
            .build();
        aws_sdk_s3::Client::from_conf(s3_config)
    });

    runtime
        .block_on(client.create_bucket().bucket(BUCKET).send())
        .unwrap();

    let store = S3ObjectStore::from_client(client.clone(), BUCKET, "studies", runtime.clone());

    let key = "1.2.3/1.2.3.4/1.2.3.4.5.dcm";
    let bytes = b"synthetic dicom-ish bytes";
    store.put(key, bytes).unwrap();
    // a retry re-putting the same instance must be a harmless overwrite
    store.put(key, bytes).unwrap();

    // the fetch leg reads the same bytes back through the port
    let fetched = store.get(key).unwrap();
    assert_eq!(&fetched[..], &bytes[..]);

    // missing keys fail cleanly, invalid keys before any network traffic
    assert!(store.get("1.2.3/1.2.3.4/nothing-here.dcm").is_err());
    assert!(store.get("../evil.dcm").is_err());
    assert!(store.put("../evil.dcm", bytes).is_err());
}
