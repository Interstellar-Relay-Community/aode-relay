use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "relay", about = "An activitypub relay")]
pub(crate) struct Args {
    #[structopt(short, help = "A list of domains that should be blocked")]
    blocks: Vec<String>,

    #[structopt(short, help = "A list of domains that should be allowed")]
    allowed: Vec<String>,

    #[structopt(short, long, help = "Undo allowing or blocking domains")]
    undo: bool,
}

impl Args {
    pub(crate) fn new() -> Self {
        Self::from_args()
    }

    pub(crate) fn blocks(&self) -> &[String] {
        &self.blocks
    }

    pub(crate) fn allowed(&self) -> &[String] {
        &self.allowed
    }

    pub(crate) fn undo(&self) -> bool {
        self.undo
    }
}
