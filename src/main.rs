use ethers::contract::ContractFactory;
use ethers::core::utils::Anvil;
use ethers::middleware::SignerMiddleware;
use ethers::providers::{Http, Provider};
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

const DEFAULT_MNEMONIC: &str = "test test test test test test test test test test test junk";

async fn deploy_contracts(
    provider: Provider<Http>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Deploying contracts to localhost{}", port);

    // configuring paths for the contract compilation
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contracts");
    let paths = ProjectPathsConfig::builder()
        .root(&root)
        .sources(&root)
        .build()?;

    // Obtener la instancia del proyecto solc usando las rutas anteriores
    let project = Project::builder()
        .paths(paths)
        .ephemeral()
        .no_artifacts()
        .build()?;

    // Compilar el proyecto y obtener los artefactos
    let output = project.compile()?;
    let contract = output
        .find_first("MockGateway")
        .expect("could not find contract")
        .clone();

    let (abi, bytecode, _) = contract.into_parts();

    // Crear la wallet a partir de la mnemotecnia
    let wallet = MnemonicBuilder::<English>::default()
        .phrase(DEFAULT_MNEMONIC)
        .build()?;

    println!("Wallet address: {}", wallet.address());

    let client = SignerMiddleware::new(provider, wallet);
    let client = Arc::new(client);

    // Crear una fábrica que se utilizará para desplegar instancias del contrato
    let factory = ContractFactory::new(abi.unwrap(), bytecode.unwrap(), client.clone());

    // Desplegarlo con los argumentos del constructor
    let contract = factory.deploy(())?.send().await?;

    // Obtener la dirección del contrato
    let addr = contract.address();

    println!("Contract deployed to: {}", addr);

    Ok(())
}

async fn start_anvil_instance(port: u16, shutdown_signal: Arc<AtomicBool>) -> Provider<Http> {
    task::spawn_blocking(move || {
        let _anvil_instance = Anvil::new()
            .port(port)
            .mnemonic(DEFAULT_MNEMONIC)
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

    Provider::<Http>::try_from(format!("http://localhost:{}", port))
        .expect("Failed to connect to Anvil")
}

async fn run_anvil_and_deploy(shutdown_signal: Arc<AtomicBool>) {
    let port1: u16 = 8545;
    let port2: u16 = 8546;

    // start anvil instances and get the providers
    let (provider1, provider2) = tokio::join!(
        start_anvil_instance(port1, shutdown_signal.clone()),
        start_anvil_instance(port2, shutdown_signal.clone())
    );

    // deploy contracts to each anvil instance
    if let Err(e) = deploy_contracts(provider1, port1).await {
        eprintln!("Error deploying contracts with provider1: {}", e);
    }

    if let Err(e) = deploy_contracts(provider2, port2).await {
        eprintln!("Error deploying contracts with provider2: {}", e);
    }
}

async fn graceful_shutdown(shutdown_signal: Arc<AtomicBool>) {
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
        _ = run_anvil_and_deploy(shutdown_signal.clone()) => {
            println!("Anvil instances started and contracts deployed. Waiting for SIGTERM to shutdown.");
        },
        _ = graceful_shutdown(shutdown_signal) => {
            println!("Shutdown signal received, exiting.");
        },
    }
}
