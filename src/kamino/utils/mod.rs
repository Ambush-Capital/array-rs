pub mod fraction;
pub mod consts;
pub mod borrow_rate_curve;
pub mod serde_helpers;
pub mod idl_parser;
pub mod errors;




/* 
fn get_slot_duration(rpc_client: &RpcClient) -> Duration {
    let start_time = Instant::now();
    let first_slot = rpc_client.get_slot().unwrap();
    
    // Wait longer to capture multiple slots
    thread::sleep(Duration::from_secs(4)); // Increased wait time
    
    let second_slot = rpc_client.get_slot().unwrap();
    let elapsed = start_time.elapsed();
    
    let slot_difference = second_slot.saturating_sub(first_slot);
    Duration::from_nanos((elapsed.as_nanos() as u64) / slot_difference)
}
*/