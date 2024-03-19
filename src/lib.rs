use anyhow::{bail, Context};

pub mod merge;

pub(crate) trait ToAnyhow {
    fn to_anyhow(self) -> anyhow::Result<std::process::Output>;
}

impl ToAnyhow for Result<std::process::Output, std::io::Error> {
    fn to_anyhow(self) -> anyhow::Result<std::process::Output> {
        let output = self.with_context(|| "failed to execute process")?;

        if !output.status.success() {
            bail!(
                "failed to run command: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(output)
    }
}
