use ructe::Ructe;

fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();

    let mut ructe = Ructe::from_env()?;
    let mut statics = ructe.statics()?;
    statics.add_sass_file("scss/index.scss")?;
    ructe.compile_templates("templates")?;

    Ok(())
}
