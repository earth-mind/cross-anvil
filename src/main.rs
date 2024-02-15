use ethers::contract::ContractFactory;
use ethers::core::utils::Anvil;
use ethers::middleware::SignerMiddleware;
use ethers::providers::{Http, Middleware, Provider};
use ethers::signers::coins_bip39::English;
use ethers::signers::MnemonicBuilder;
use ethers::signers::Signer;
use ethers::solc::{Artifact, Project, ProjectPathsConfig};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::signal::unix::{signal, SignalKind};
use tokio::task;
use tokio::time::{sleep, Duration};

const DEFAULT_MNEMONIC: &str = "test test test test test test test test test test test junk";
const MAX_RETRIES: u16 = 10;
const DEFAULT_CHAIN_ID: u16 = 1337;

async fn deploy_contracts(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    println!("Waiting for Anvil on port {} to be ready...", port);

    let provider = Provider::<Http>::try_from(format!("http://localhost:{}", port))?;
    for _ in 0..MAX_RETRIES {
        if provider.get_chainid().await.is_ok() {
            println!("Anvil on port {} is ready!", port);
            break;
        } else {
            sleep(Duration::from_secs(1)).await;
            println!("Anvil on port {} is not ready yet. Retrying...", port);
        }
    }

    println!("Deploying contracts to localhost:{}", port);

    // configuring paths for the contract compilation
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contracts");
    let paths = ProjectPathsConfig::builder()
        .root(&root)
        .sources(&root)
        .build()?;

    let project = Project::builder()
        .paths(paths)
        .ephemeral()
        .no_artifacts()
        .build()?;

    // compile the contract and get the ABI and bytecode
    let output = project.compile()?;
    let contract = output
        .find_first("MockGateway")
        .expect("could not find contract")
        .clone();

    println!("Contract compiled successfully");

    let (abi, bytecode, _) = contract.into_parts();

    // create wallet from default mnemonic
    let wallet = MnemonicBuilder::<English>::default()
        .phrase(DEFAULT_MNEMONIC)
        .build()?;

    println!("Wallet address: {}", wallet.address());

    let client = SignerMiddleware::new(provider, wallet.with_chain_id(DEFAULT_CHAIN_ID));
    let client = Arc::new(client);

    // create a factory to deploy instances of the contract
    let factory = ContractFactory::new(abi.unwrap(), bytecode.unwrap(), client.clone());

    println!("Deploying contract...");

    match factory.deploy(())?.send().await {
        Ok(contract) => {
            let addr = contract.address();
            println!("Contract deployed to: {}", addr);
        }
        Err(e) => {
            eprintln!("Failed to deploy contract: {}", e);
            return Err(e.into()); // convert the error to match the return type
        }
    }

    Ok(())
}

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
        start_anvil_instance(port2, shutdown_signal.clone()),
        deploy_contracts(port1),
        deploy_contracts(port2)
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
