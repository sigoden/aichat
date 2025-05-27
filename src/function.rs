use crate::{
    config::{Agent, Config, GlobalConfig},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const PATH_SEP: &str = ";";
#[cfg(not(windows))]
const PATH_SEP: &str = ":";

pub fn eval_tool_calls(config: &GlobalConfig, mut calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }
    let mut is_all_null = true;
    for call in calls {
        let mut result = call.eval(config)?;
        if result.is_null() {
            result = json!("DONE");
        } else {
            is_all_null = false;
        }
        output.push(ToolResult::new(call, result));
    }
    if is_all_null {
        output = vec![];
    }
    Ok(output)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResult {
    pub call: ToolCall,
    pub output: Value,
}

impl ToolResult {
    pub fn new(call: ToolCall, output: Value) -> Self {
        Self { call, output }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Functions {
    declarations: Vec<FunctionDeclaration>,
}

impl Functions {
    pub fn init(declarations_path: &Path) -> Result<Self> {
        let declarations: Vec<FunctionDeclaration> = if declarations_path.exists() {
            let ctx = || {
                format!(
                    "Failed to load functions at {}",
                    declarations_path.display()
                )
            };
            let content = fs::read_to_string(declarations_path).with_context(ctx)?;
            serde_json::from_str(&content).with_context(ctx)?
        } else {
            vec![]
        };

        Ok(Self { declarations })
    }

    pub fn find(&self, name: &str) -> Option<&FunctionDeclaration> {
        self.declarations.iter().find(|v| v.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.declarations.iter().any(|v| v.name == name)
    }

    pub fn declarations(&self) -> &[FunctionDeclaration] {
        &self.declarations
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)]
    pub agent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, JsonSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<JsonSchema>>,
    #[serde(rename = "anyOf", skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<JsonSchema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_value: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl JsonSchema {
    pub fn is_empty_properties(&self) -> bool {
        match &self.properties {
            Some(v) => v.is_empty(),
            None => true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    pub id: Option<String>,
}

type CallConfig = (String, String, Vec<String>, HashMap<String, String>);

impl ToolCall {
    pub fn dedup(calls: Vec<Self>) -> Vec<Self> {
        let mut new_calls = vec![];
        let mut seen_ids = HashSet::new();

        for call in calls.into_iter().rev() {
            if let Some(id) = &call.id {
                if !seen_ids.contains(id) {
                    seen_ids.insert(id.clone());
                    new_calls.push(call);
                }
            } else {
                new_calls.push(call);
            }
        }

        new_calls.reverse();
        new_calls
    }

    pub fn new(name: String, arguments: Value, id: Option<String>) -> Self {
        Self {
            name,
            arguments,
            id,
        }
    }

    pub fn eval(&self, config: &GlobalConfig) -> Result<Value> {
        if self.name == "execute_shell_command" {
            let command_str = match self.arguments.get("command").and_then(|v| v.as_str()) {
                Some(cmd) => cmd.to_string(),
                None => bail!("'execute_shell_command' requires a 'command' string argument."),
            };
            return Ok(json!({
                "command_string": command_str,
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            }));
        }

        let (call_name, cmd_name, mut cmd_args, envs) = match &config.read().agent {
            Some(agent) => self.extract_call_config_from_agent(config, agent)?,
            None => self.extract_call_config_from_config(config)?,
        };

        let json_data = if self.arguments.is_object() {
            self.arguments.clone()
        } else if let Some(arguments) = self.arguments.as_str() {
            let arguments: Value = serde_json::from_str(arguments).map_err(|_| {
                anyhow!("The call '{call_name}' has invalid arguments: {arguments}")
            })?;
            arguments
        } else {
            bail!(
                "The call '{call_name}' has invalid arguments: {}",
                self.arguments
            );
        };

        cmd_args.push(json_data.to_string());

        let output = match run_llm_function(cmd_name, cmd_args, envs)? {
            Some(contents) => serde_json::from_str(&contents)
                .ok()
                .unwrap_or_else(|| json!({"output": contents})),
            None => Value::Null,
        };

        Ok(output)
    }

    fn extract_call_config_from_agent(
        &self,
        config: &GlobalConfig,
        agent: &Agent,
    ) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match agent.functions().find(&function_name) {
            Some(function) => {
                let agent_name = agent.name().to_string();
                if function.agent {
                    Ok((
                        format!("{agent_name}-{function_name}"),
                        agent_name,
                        vec![function_name],
                        agent.variable_envs(),
                    ))
                } else {
                    Ok((
                        function_name.clone(),
                        function_name,
                        vec![],
                        Default::default(),
                    ))
                }
            }
            None => self.extract_call_config_from_config(config),
        }
    }

    fn extract_call_config_from_config(&self, config: &GlobalConfig) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match config.read().functions.contains(&function_name) {
            true => Ok((
                function_name.clone(),
                function_name,
                vec![],
                Default::default(),
            )),
            false => bail!("Unexpected call: {function_name} {}", self.arguments),
        }
    }
}

pub fn run_llm_function(
    cmd_name: String,
    cmd_args: Vec<String>,
    mut envs: HashMap<String, String>,
) -> Result<Option<String>> {
    let prompt = format!("Call {cmd_name} {}", cmd_args.join(" "));

    let mut bin_dirs: Vec<PathBuf> = vec![];
    if cmd_args.len() > 1 {
        let dir = Config::agent_functions_dir(&cmd_name).join("bin");
        if dir.exists() {
            bin_dirs.push(dir);
        }
    }
    bin_dirs.push(Config::functions_bin_dir());
    let current_path = std::env::var("PATH").context("No PATH environment variable")?;
    let prepend_path = bin_dirs
        .iter()
        .map(|v| format!("{}{PATH_SEP}", v.display()))
        .collect::<Vec<_>>()
        .join("");
    envs.insert("PATH".into(), format!("{prepend_path}{current_path}"));

    let temp_file = temp_file("-eval-", "");
    envs.insert("LLM_OUTPUT".into(), temp_file.display().to_string());

    #[cfg(windows)]
    let cmd_name = polyfill_cmd_name(&cmd_name, &bin_dirs);
    if *IS_STDOUT_TERMINAL {
        println!("{}", dimmed_text(&prompt));
    }
    let exit_code = run_command(&cmd_name, &cmd_args, Some(envs))
        .map_err(|err| anyhow!("Unable to run {cmd_name}, {err}"))?;
    if exit_code != 0 {
        bail!("Tool call exit with {exit_code}");
    }
    let mut output = None;
    if temp_file.exists() {
        let contents =
            fs::read_to_string(temp_file).context("Failed to retrieve tool call output")?;
        if !contents.is_empty() {
            output = Some(contents);
        }
    };
    Ok(output)
}

#[cfg(windows)]
fn polyfill_cmd_name<T: AsRef<Path>>(cmd_name: &str, bin_dir: &[T]) -> String {
    let cmd_name = cmd_name.to_string();
    if let Ok(exts) = std::env::var("PATHEXT") {
        for name in exts.split(';').map(|ext| format!("{cmd_name}{ext}")) {
            for dir in bin_dir {
                let path = dir.as_ref().join(&name);
                if path.exists() {
                    return name.to_string();
                }
            }
        }
    }
    cmd_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, GlobalConfig, WorkingMode};
    use parking_lot::RwLock;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::tempdir;

    // Helper to create a GlobalConfig for tests, ensuring functions are loaded.
    fn create_test_config() -> Result<GlobalConfig> {
        // Create a temporary directory for functions to ensure isolation if needed,
        // or rely on the project's functions directory if scripts are stable.
        // For this test, we assume `functions/functions.json` and the scripts in `functions/bin`
        // are correctly set up in the project directory as per previous steps.

        let temp_dir = tempdir()?;
        let config_dir = temp_dir.path().join(".config/aichat");
        fs::create_dir_all(&config_dir)?;

        // Create functions dir and bin dir relative to where cargo test is run (project root)
        let project_root_functions_dir = PathBuf::from("functions");
        let project_root_functions_bin_dir = project_root_functions_dir.join("bin");

        // Check if the actual functions directory and scripts exist.
        // If not, these tests will fail, which is expected as they depend on prior setup.
        if !project_root_functions_dir.exists() || !project_root_functions_bin_dir.exists() {
            panic!("'functions' directory and 'functions/bin' must exist for these tests. Run setup steps first.");
        }
        
        // Create a minimal config that points to the project's functions directory
        let mut cfg = Config::default();
        
        // Override functions_dir to point to the project's actual 'functions' directory
        // This requires a bit of a workaround as Config methods for paths are not easily overridable
        // for tests without altering the Config struct itself.
        // Instead, we ensure that when Config::init calls load_functions, it finds our functions.json.
        // The `Config::init` and `load_functions` use `Config::functions_file()`.
        // We will mock `Config::functions_file()` effectively by placing a temporary config dir
        // and copying our project `functions` folder into a location relative to it,
        // or more simply, ensuring the test runner's current directory allows `Config::functions_file()`
        // to resolve to `project_root/functions/functions.json`.
        // The easiest way is to rely on the default path resolution of Config::functions_dir()
        // which is `Config::local_path(FUNCTIONS_DIR_NAME)`.
        // We'll let Config use its default logic, assuming tests are run from project root.

        // To ensure `Config::functions_file()` and `Config::functions_bin_dir()` resolve correctly
        // without complex mocking, we rely on the tests being run from the project root,
        // and that `Config::local_path` will form paths like `target/debug/deps/.../functions` if we don't
        // guide it.
        // A simpler approach for testing `ToolCall::eval` is to ensure the `GlobalConfig` it receives
        // has its `functions` field populated correctly and that `run_llm_function` can find the scripts.
        // `run_llm_function` uses `Config::functions_bin_dir()` and `Config::agent_functions_dir()`.
        
        // Let's try to initialize config and then manually load functions from our project path.
        // This is a bit of a hack because Config internally manages its paths.
        // The most direct way to test `ToolCall::eval` is to have a `GlobalConfig`
        // where `config.read().functions` is what we expect and script paths are resolvable.

        // We'll use a simplified Config initialization for tests.
        let mut test_config = Config {
            working_mode: WorkingMode::Cmd, // or Repl, doesn't matter much for these tests
            // We need to ensure cfg.functions is loaded.
            // Config::init calls load_functions. Let's mimic that part.
            functions: Functions::init(&project_root_functions_dir.join("functions.json"))?,
            ..Default::default()
        };

        let global_config = Arc::new(RwLock::new(test_config));
        Ok(global_config)
    }

    #[test]
    fn test_web_search_tool_call() -> Result<()> {
        let config = create_test_config()?;
        let query = "test query for web search";
        let arguments = json!({"query": query});
        let tool_call = ToolCall::new("web_search".to_string(), arguments, None);

        let result = tool_call.eval(&config)?;

        assert!(result.is_object(), "Result should be a JSON object");
        let results_arr = result.get("results").expect("Should have 'results' field");
        assert!(results_arr.is_array(), "'results' should be an array");
        assert_eq!(results_arr.as_array().unwrap().len(), 2, "Should have 2 dummy results");

        let first_result = &results_arr.as_array().unwrap()[0];
        assert_eq!(
            first_result.get("title").unwrap().as_str().unwrap(),
            format!("Dummy Result 1 for '{}'", query)
        );
        Ok(())
    }

    #[test]
    fn test_execute_shell_safelisted_pwd() -> Result<()> {
        let config = create_test_config()?;
        let command = "pwd";
        let arguments = json!({"command": command});
        let tool_call = ToolCall::new("execute_shell_command".to_string(), arguments, None);

        let result = tool_call.eval(&config)?;

        assert_eq!(
            result,
            json!({
                "command_string": "pwd",
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            })
        );
        Ok(())
    }

    #[test]
    fn test_execute_shell_safelisted_echo() -> Result<()> {
        let config = create_test_config()?;
        let command = "echo hello"; // This specific phrase is safelisted
        let arguments = json!({"command": command});
        let tool_call = ToolCall::new("execute_shell_command".to_string(), arguments, None);

        let result = tool_call.eval(&config)?;
        assert_eq!(
            result,
            json!({
                "command_string": "echo hello",
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            })
        );
        Ok(())
    }
    
    #[test]
    fn test_execute_shell_safelisted_echo_quoted() -> Result<()> {
        let config = create_test_config()?;
        let command = "echo \"hello world\""; // Test safelisted echo with quotes
        let arguments = json!({"command": command});
        let tool_call = ToolCall::new("execute_shell_command".to_string(), arguments, None);

        let result = tool_call.eval(&config)?;
        assert_eq!(
            result,
            json!({
                "command_string": "echo \"hello world\"",
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            })
        );
        Ok(())
    }


    #[test]
    fn test_execute_shell_non_safelisted() -> Result<()> {
        let config = create_test_config()?;
        let command = "cat /etc/shadow"; // Clearly not safelisted
        let arguments = json!({"command": command});
        let tool_call = ToolCall::new("execute_shell_command".to_string(), arguments, None);

        let result = tool_call.eval(&config)?;
        assert_eq!(
            result,
            json!({
                "command_string": "cat /etc/shadow",
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            })
        );
        Ok(())
    }

    #[test]
    fn test_execute_shell_malicious_attempt_within_safelisted_echo() -> Result<()> {
        let config = create_test_config()?;
        let config = create_test_config()?;
        // This test now only checks if the command string is correctly passed through.
        // The actual execution and safelisting logic is bypassed in ToolCall::eval.
        let command_to_test = "echo \"UID is $(id -u)\""; 
        let arguments = json!({"command": command_to_test});
        let tool_call = ToolCall::new("execute_shell_command".to_string(), arguments, None);

        let result = tool_call.eval(&config)?;
        
        assert_eq!(
            result,
            json!({
                "command_string": command_to_test,
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            })
        );

        // The second part of the original test, which tested an explicitly blocked command,
        // is also now just about passing the command string.
        let command_blocked = "echo \"hello ; whoami\"";
        let arguments_blocked = json!({"command": command_blocked});
        let tool_call_blocked = ToolCall::new("execute_shell_command".to_string(), arguments_blocked, None);
        let result_blocked = tool_call_blocked.eval(&config)?;
        assert_eq!(
            result_blocked,
            json!({
                "command_string": command_blocked,
                "message": "Command ready for manual execution. Direct execution is disabled for security."
            })
        );

        Ok(())
    }
}
