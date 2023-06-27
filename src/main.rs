use std::path::Path;
use anyhow::anyhow;
use clap::Parser;
use havocompare::{compare_folders, get_schema, validate_config};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

const DEFAULT_REPORT_FOLDER: &str = "report";

#[derive(clap::Subcommand)]
enum Commands {
    /// Compare two folders using a config file
    Compare {
        /// Nominal data folder
        nominal: String,
        /// Actual data folder
        actual: String,
        /// Path to compare config YAML
        compare_config: String,
        /// Optional: Folder to store the report to, if not set the default location will be chosen.
        #[arg(short, long = "report_path", default_value_t = DEFAULT_REPORT_FOLDER.to_string())]
        report_config: String,
        /// Open the report immediately after comparison
        #[arg(short, long)]
        open: bool
    },

    /// Export the JsonSchema for the config files
    Schema,

    /// Validate config yaml
    Validate { compare_config: String },
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Arguments {
    #[clap(short, long)]
    /// print debug information about the run
    verbose: bool,
    #[clap(subcommand)]
    /// choose the command to run
    command: Commands,
}

fn main() -> Result<(), vg_errortools::MainError> {
    let args = Arguments::parse();
    let level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    // enable colors on windows cmd.exe
    // does not fail on powershell, even though powershell can do colors without this
    // will fail on jenkins/qa tough, that's why we need to ignore the result
    let _ = enable_ansi_support::enable_ansi_support();

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    match args.command {
        Commands::Schema => {
            println!(
                "{}",
                get_schema().expect("Error occurred writing json schema")
            );
            Ok(())
        }
        Commands::Compare {
            compare_config,
            nominal,
            actual,
            report_config,
            open
        } => {
            let report_path = Path::new(report_config.as_str());
            let result =
                compare_folders(nominal, actual, compare_config, report_path)?;
            if open {
                info!("Opening report");
                opener::open(report_path.join("index.html")).expect("Could not open report!");
            }
            if result {
                Ok(())
            } else {
                Err(anyhow!("Comparison failed!").into())
            }
        }
        Commands::Validate { compare_config } => {
            if validate_config(compare_config) {
                Ok(())
            } else {
                Err(anyhow!("Validation failed!").into())
            }
        }
    }
}
