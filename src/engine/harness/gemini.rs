//! Gemini CLI: `gemini -p` with stream-json output. Not installed where this
//! was written, so every flag here comes from its documentation; see
//! `docs/HARNESS_MAPPING.md`.

use super::{planned, prompt_arg, signed_in, strs, AuthStatus, Harness, Mode, Plan, TurnInput};
use crate::engine::{attach::Delivery, parse::Protocol};

pub struct Gemini;

impl Harness for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    /// Line icon: four-point sparkle.
    fn glyph(&self) -> Option<&'static str> {
        Some("M8 1.5C8.6 5.5 10.5 7.4 14.5 8C10.5 8.6 8.6 10.5 8 14.5C7.4 10.5 5.5 8.6 1.5 8C5.5 7.4 7.4 5.5 8 1.5Z")
    }
    fn binary(&self) -> &'static str {
        "gemini"
    }

    fn protocol(&self) -> Protocol {
        Protocol::Gemini
    }

    fn modes(&self) -> &'static [Mode] {
        &Mode::ALL
    }

    /// Gemini has no image flag, so every attachment goes by path: its
    /// directories are included and the prompt lists the files.
    fn plan(&self, input: &TurnInput) -> Plan {
        let files = Delivery::split(input.files, |_| false);
        let mut a = Vec::new();
        if let Some(model) = input.model {
            a.extend(strs(&["-m", model]));
        }
        let approval = match input.mode {
            Mode::Auto => "auto_edit",
            Mode::Plan | Mode::Ask => "plan",
        };
        a.extend(strs(&["--approval-mode", approval]));
        // Headless Gemini (0.64.0) exits in a folder the user never trusted, and outside a trusted
        // folder it also overrides `--approval-mode` to `default`. ArchonUI chooses the folder, as
        // `claude -p` does, so the turn trusts it.
        a.push("--skip-trust".into());
        for dir in files.dirs() {
            a.extend(["--include-directories".into(), dir.display().to_string()]);
        }
        if let Some(id) = input.session {
            a.extend(strs(&["--resume", id]));
        }
        a.extend(strs(&["--output-format", "stream-json"]));
        // Plan and Ask share `--approval-mode plan`, so Plan also asks for a plan in the prompt.
        a.extend(["-p".into(), prompt_arg(&planned(input.mode, files.prompt(input.prompt)))]);
        Plan { args: a, ..Default::default() }
    }

    fn auth_status(&self) -> AuthStatus {
        signed_in(&["GEMINI_API_KEY", "GOOGLE_API_KEY"], &[".gemini/oauth_creds.json"])
    }
}
