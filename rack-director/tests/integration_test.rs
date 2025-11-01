use crate::common::start_rack_director;

mod common;

#[tokio::test]
async fn test() {
    let rack_director = start_rack_director().await.expect("rack director start");
    assert!(true);
}
