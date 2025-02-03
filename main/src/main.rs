#![allow(clippy::empty_line_after_doc_comments)]

use aggregator::LendingMarketAggregator;

pub mod aggregator;
pub mod kamino;
pub mod marginfi;
pub mod save;

fn main() {
    env_logger::init();

    // AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN
    let mut aggregator = LendingMarketAggregator::new();
    let _ = aggregator.load_markets();
    aggregator.print_markets();
}
