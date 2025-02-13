# Makefile for ARRAY-RS Project

# Name of the SQLite database file
DB_FILE = solana_lending_markets.db

# Directories for the two workspaces
BLOCKCHAIN_DIR = blockchain
WORKER_DIR = worker
API_DIR = api

# Include environment variables from a .env file if present.
# Create a .env file (and add it to .gitignore) with a line like:
#   RPC_URL=https://mainnet.helius-rpc.com/?api-key=your_api_key
-include .env

.PHONY: create-db delete-db run-chain-api run-worker

	@echo "Checking for rustup..."
	@if ! command -v rustup > /dev/null; then \
		echo "rustup not found. Installing rustup and Rust toolchain..."; \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
		source $$HOME/.cargo/env; \
	else \
		echo "rustup is already installed."; \
	fi
	@echo "Checking for sqlite3..."
	@if ! command -v sqlite3 > /dev/null; then \
		echo "Installing sqlite3..."; \
		brew install sqlite3; \
	else \
		echo "sqlite3 is already installed."; \
	fi
	@echo "Checking for tmux..."
	@if ! command -v tmux > /dev/null; then \
		echo "Installing tmux..."; \
		brew install tmux; \
	else \
		echo "tmux is already installed."; \
	fi

# Create the SQLite database only if it doesn't exist.
create-db:
	@echo "Current directory: $$(pwd)"
	@if [ ! -f $(DB_FILE) ]; then \
		echo "Creating SQLite database: $(DB_FILE) in $$(pwd)"; \
		echo "CREATE TABLE IF NOT EXISTS tmp_table (id INTEGER);" | sqlite3 $(DB_FILE); \
	else \
		echo "Database $(DB_FILE) already exists in $$(pwd)"; \
	fi

# Delete the SQLite database if it exists.
delete-db:
	@if [ -f $(DB_FILE) ]; then \
		echo "Deleting SQLite database: $(DB_FILE)"; \
		rm $(DB_FILE); \
	else \
		echo "Database $(DB_FILE) does not exist."; \
	fi

# Run the chain-api from the blockchain workspace with RPC_URL set.
run-chain-api:
	@if [ -z "$(RPC_URL)" ]; then \
		echo "Error: RPC_URL is not set. Please set it in your .env file."; \
		exit 1; \
	fi
	cd $(BLOCKCHAIN_DIR) && RPC_URL=$(RPC_URL) cargo run -p chain-api

# Run the worker in the worker workspace.
run-worker:
	cd $(WORKER_DIR) && DB_FILE=$(DB_FILE) cargo run

# Run the API in the api workspace.
run-api:
	cd $(API_DIR) && DB_FILE=$(DB_FILE) cargo run

# Dev target: create DB then launch tmux to run chain-api and worker concurrently.
dev: create-db
	@echo "Starting tmux session 'array-rs' with run-chain-api and run-worker..."
	tmux new-session -d -s array-rs "make run-chain-api"
	tmux split-window -h -t array-rs "make run-worker"
	tmux split-window -v -t array-rs "make run-api"
	tmux select-layout -t array-rs tiled
	tmux attach-session -t array-rs	

dev-build:
	cd $(BLOCKCHAIN_DIR) && cargo build
	cd $(WORKER_DIR) && cargo build
	cd $(API_DIR) && cargo build