use serde_json::Value;
use std::fs;

pub fn parse_idl(idl_path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let idl_path = idl_path.unwrap_or("idls/kamino_lending.json");
    let idl_data = fs::read_to_string(idl_path)?;
    let idl_json: Value = serde_json::from_str(&idl_data)?;

    println!("Program Name: {:?}", idl_json["name"]);
    
    println!("\nInstructions:");
    if let Some(instructions) = idl_json["instructions"].as_array() {
        for instr in instructions {
            println!(" - {}", instr["name"]);
        }
    }

    println!("\nAccounts:");
    if let Some(accounts) = idl_json["accounts"].as_array() {
        for account in accounts {
            println!(" - {}", account["name"]);
        }
    }

    Ok(())
}