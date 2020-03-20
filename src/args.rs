use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "relay", about = "An activitypub relay")]
pub struct Args {
    #[structopt(short, help = "A list of domains that should be blocked")]
    blocks: Vec<String>,

    #[structopt(short, help = "A list of domains that should be whitelisted")]
    whitelists: Vec<String>,

    #[structopt(short, help = "Undo whitelisting or blocking these domains")]
    undo: bool,
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
}
