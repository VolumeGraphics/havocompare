use clap::Parser;
use havocompare::get_schema;
use tracing::Level;
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
        /// Optional Path to report file
        #[arg(short, long = "report_path", default_value_t = DEFAULT_REPORT_FOLDER.to_string())]
        report_config: String,
    },

    /// Export the JsonSchema for the config files
    Schema,
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

fn main() {
    let args = Arguments::parse();
    let level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    // enable colors on windows cmd.exe
    // does not fail on powershell, even though powershell can do colors without this
    // will fail on jenkins/qa tough, that's why we need to ignore the result
    #[cfg(windows)]
    let _ = ansi_term::enable_ansi_support();

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    match args.command {
        Commands::Schema => {
            println!("{}", get_schema());
            std::process::exit(0);
        }
        Commands::Compare {            
            compare_config,
            nominal,
            actual,
            report_config
        } => {
            let result = havocompare::compare_folders(
                nominal,
                actual,
                compare_config,
                report_config,
            );
            if result {
                std::process::exit(0);
            } else {
                std::process::exit(1);
            }
        }
    };
}
