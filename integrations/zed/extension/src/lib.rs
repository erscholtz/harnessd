use zed_extension_api as zed;

struct HarnessdExtension;

impl zed::Extension for HarnessdExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let command = worktree
            .which("harnessd")
            .or_else(|| repo_debug_binary(worktree))
            .ok_or_else(|| {
                "Could not find harnessd. Build this repo or put harnessd on PATH.".to_string()
            })?;

        Ok(zed::Command {
            command,
            args: vec!["lsp".to_string()],
            env: worktree.shell_env(),
        })
    }
}

fn repo_debug_binary(worktree: &zed::Worktree) -> Option<String> {
    let root = worktree.root_path();

    #[cfg(target_os = "windows")]
    let path = format!("{root}\\target\\debug\\harnessd.exe");

    #[cfg(not(target_os = "windows"))]
    let path = format!("{root}/target/debug/harnessd");

    Some(path)
}

zed::register_extension!(HarnessdExtension);
