use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "relay", about = "An activitypub relay")]
pub struct Args {
    #[structopt(short, help = "A list of domains that should be blocked")]
    blocks: Vec<String>,

    #[structopt(short, help = "A list of domains that should be allowed")]
    allowed: Vec<String>,

    #[structopt(short, long, help = "Undo allowing or blocking domains")]
    undo: bool,
}

impl Args {
    pub fn new() -> Self {
        Self::from_args()
    }

    pub fn blocks(&self) -> &[String] {
        &self.blocks
    }

    pub fn allowed(&self) -> &[String] {
        &self.allowed
    }

    pub fn undo(&self) -> bool {
        self.undo
    }
}
