use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::Client;

pub async fn client(region: &str, endpoint_url: Option<&str>) -> Client {
    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(region.to_owned()))
        .load()
        .await;

    let mut config = aws_sdk_s3::config::Builder::from(&shared_config).force_path_style(true);
    if let Some(endpoint_url) = endpoint_url {
        config = config.endpoint_url(endpoint_url);
    }

    Client::from_conf(config.build())
}
