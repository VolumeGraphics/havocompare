use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

fn main() {
    // enable colors on windows cmd.exe
    // does not fail on powershell, even though powershell can do colors without this
    // will fail on jenkins/qa tough, that's why we need to ignore the result
    let _ = enable_ansi_support::enable_ansi_support();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    info!("!Print-Args!");
    for arg in std::env::args() {
        info!("Argument: {arg}");
    }
}
