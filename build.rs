fn main() -> Result<(), Box<dyn std::error::Error>> {
    topcoat::tailwind::BuildConfig::new()
        .input("assets/styles.css")
        .render()?;
    Ok(())
}
