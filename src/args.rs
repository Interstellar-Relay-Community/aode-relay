use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "relay", about = "An activitypub relay")]
pub struct Args {
    #[structopt(short, help = "A list of domains that should be blocked")]
    blocks: Vec<String>,

    #[structopt(short, help = "A list of domains that should be whitelisted")]
    whitelists: Vec<String>,

    #[structopt(short, long, help = "Undo whitelisting or blocking domains")]
    undo: bool,

    #[structopt(
        short,
        long,
        help = "Only process background jobs, do not start the relay server"
    )]
    jobs_only: bool,

    #[structopt(
        short,
        long,
        help = "Only run the relay server, do not process background jobs"
    )]
    no_jobs: bool,
}

impl Args {
    pub fn new() -> Self {
        Self::from_args()
    }

    pub fn blocks(&self) -> &[String] {
        &self.blocks
    }

    pub fn whitelists(&self) -> &[String] {
        &self.whitelists
    }

    pub fn undo(&self) -> bool {
        self.undo
    }

    pub fn jobs_only(&self) -> bool {
        self.jobs_only
    }

    pub fn no_jobs(&self) -> bool {
        self.no_jobs
    }
}
