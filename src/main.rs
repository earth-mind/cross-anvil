use ethers::core::utils::Anvil;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::signal::unix::{signal, SignalKind};
use tokio::task;

const DEFAULT_MNEMONIC: &str = "test test test test test test test test test test test junk";
const DEFAULT_CHAIN_ID: u16 = 1337;

async fn start_anvil_instance(port: u16, shutdown_signal: Arc<AtomicBool>) {
    task::spawn_blocking(move || {
        let _anvil_instance = Anvil::new()
            .port(port)
            .mnemonic(DEFAULT_MNEMONIC)
            .chain_id(DEFAULT_CHAIN_ID)
            .args(vec!["--base-fee", "100"])
            .spawn();

        println!("Anvil instance running on port: {}", port);

        // Polling for shutdown signal
        while !shutdown_signal.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        println!("Shutting down Anvil instance on port: {}", port);
    })
    .await
    .expect("Failed to execute Anvil instance");
}

async fn run_anvil_and_deploy(
    shutdown_signal: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let port1: u16 = 8545;
    let port2: u16 = 8546;

    let _ = tokio::join!(
        start_anvil_instance(port1, shutdown_signal.clone()),
        start_anvil_instance(port2, shutdown_signal.clone())
    );

    Ok(())
}

async fn monitor_shutdown_signal(shutdown_signal: Arc<AtomicBool>) {
    let mut term_signal =
        signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");
    term_signal.recv().await;
    println!("SIGTERM received, initiating graceful shutdown...");
    shutdown_signal.store(true, Ordering::SeqCst);
}

#[tokio::main]
async fn main() {
    let shutdown_signal = Arc::new(AtomicBool::new(false));

    tokio::select! {
        result = run_anvil_and_deploy(shutdown_signal.clone()) => {
            match result {
                Ok(_) => println!("Anvil instances started and contracts deployed successfully."),
                Err(e) => eprintln!("An error occurred: {}", e),
            }
        },
        _ = monitor_shutdown_signal(shutdown_signal) => {
            println!("Shutdown signal received, exiting.");
        },
    }
}
