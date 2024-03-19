use anyhow::Result;
use argh::FromArgs;
use trenza::merge::merge_repositories;

#[derive(FromArgs, PartialEq, Debug)]
/// Join repositories to one monorepo.
struct Trenza {
    #[argh(subcommand)]
    cmd: Commands,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum Commands {
    Join(JoinRepoArgs),
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "join")]
/// join repositories
struct JoinRepoArgs {
    /// root directory below which to join git repositories
    #[argh(positional)]
    root: String,

    /// suffix to append to the new joined repository
    #[argh(option, default = "String::from(\"_joined\")")]
    suffix: String,

    /// branch to use for every repository
    #[argh(option)]
    branch: Option<String>,
}

fn main() -> Result<()> {
    env_logger::init();

    let cli: Trenza = argh::from_env();

    match cli.cmd {
        Commands::Join(args) => {
            merge_repositories(&args.root, &args.suffix, args.branch.as_deref())
        }
    }
}
