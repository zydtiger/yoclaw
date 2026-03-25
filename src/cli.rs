use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "yoclaw",
    about = "Telegram-first LLM agent and local skill manager"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum SkillCommands {
    Add { source: SkillSource },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Dir(String),
    Zip(String),
}

impl std::str::FromStr for SkillSource {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, source) = value
            .split_once(':')
            .ok_or_else(|| "skill source must start with `dir:` or `zip:`".to_string())?;

        if source.is_empty() {
            return Err("skill source cannot be empty".to_string());
        }

        match kind {
            "dir" => Ok(Self::Dir(source.to_string())),
            "zip" => Ok(Self::Zip(source.to_string())),
            _ => Err("skill source must start with `dir:` or `zip:`".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, SkillCommands, SkillSource};

    #[test]
    fn parse_without_subcommand_keeps_runtime_mode() {
        let cli = Cli::try_parse_from(["yoclaw"]).expect("cli should parse");
        assert!(cli.command.is_none());
    }

    #[test]
    fn parse_dir_skill_add_command() {
        let cli = Cli::try_parse_from(["yoclaw", "skill", "add", "dir:./skill-dir"])
            .expect("cli should parse");

        match cli.command {
            Some(Commands::Skill {
                command:
                    SkillCommands::Add {
                        source: SkillSource::Dir(path),
                    },
            }) => assert_eq!(path, "./skill-dir"),
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn parse_zip_skill_add_command() {
        let cli = Cli::try_parse_from(["yoclaw", "skill", "add", "zip:https://example.com/a.zip"])
            .expect("cli should parse");

        match cli.command {
            Some(Commands::Skill {
                command:
                    SkillCommands::Add {
                        source: SkillSource::Zip(path),
                    },
            }) => assert_eq!(path, "https://example.com/a.zip"),
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn reject_invalid_skill_source_prefix() {
        let error = Cli::try_parse_from(["yoclaw", "skill", "add", "file:./skill.zip"])
            .expect_err("cli should reject invalid source");

        assert!(error
            .to_string()
            .contains("skill source must start with `dir:` or `zip:`"));
    }

    #[test]
    fn reject_empty_skill_source_payload() {
        let error = Cli::try_parse_from(["yoclaw", "skill", "add", "dir:"])
            .expect_err("cli should reject empty source");

        assert!(error.to_string().contains("skill source cannot be empty"));
    }
}
