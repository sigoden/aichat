using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'aichat' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'aichat'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'aichat' {
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'Select a LLM model')
            [CompletionResult]::new('--model', '--model', [CompletionResultType]::ParameterName, 'Select a LLM model')
            [CompletionResult]::new('--prompt', '--prompt', [CompletionResultType]::ParameterName, 'Use the system prompt')
            [CompletionResult]::new('-r', '-r', [CompletionResultType]::ParameterName, 'Select a role')
            [CompletionResult]::new('--role', '--role', [CompletionResultType]::ParameterName, 'Select a role')
            [CompletionResult]::new('-s', '-s', [CompletionResultType]::ParameterName, 'Start or join a session')
            [CompletionResult]::new('--session', '--session', [CompletionResultType]::ParameterName, 'Start or join a session')
            [CompletionResult]::new('-f', '-f', [CompletionResultType]::ParameterName, 'Include files with the message')
            [CompletionResult]::new('--file', '--file', [CompletionResultType]::ParameterName, 'Include files with the message')
            [CompletionResult]::new('-w', '-w', [CompletionResultType]::ParameterName, 'Control text wrapping (no, auto, <max-width>)')
            [CompletionResult]::new('--wrap', '--wrap', [CompletionResultType]::ParameterName, 'Control text wrapping (no, auto, <max-width>)')
            [CompletionResult]::new('--save-session', '--save-session', [CompletionResultType]::ParameterName, 'Forces the session to be saved')
            [CompletionResult]::new('--serve', '--serve', [CompletionResultType]::ParameterName, 'Serve the LLM API and WebAPP')
            [CompletionResult]::new('-e', '-e', [CompletionResultType]::ParameterName, 'Execute commands in natural language')
            [CompletionResult]::new('--execute', '--execute', [CompletionResultType]::ParameterName, 'Execute commands in natural language')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Output code only')
            [CompletionResult]::new('--code', '--code', [CompletionResultType]::ParameterName, 'Output code only')
            [CompletionResult]::new('-H', '-H', [CompletionResultType]::ParameterName, 'Turn off syntax highlighting')
            [CompletionResult]::new('--no-highlight', '--no-highlight', [CompletionResultType]::ParameterName, 'Turn off syntax highlighting')
            [CompletionResult]::new('-S', '-S', [CompletionResultType]::ParameterName, 'Turns off stream mode')
            [CompletionResult]::new('--no-stream', '--no-stream', [CompletionResultType]::ParameterName, 'Turns off stream mode')
            [CompletionResult]::new('--light-theme', '--light-theme', [CompletionResultType]::ParameterName, 'Use light theme')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Display the message without sending it')
            [CompletionResult]::new('--info', '--info', [CompletionResultType]::ParameterName, 'Display information')
            [CompletionResult]::new('--list-models', '--list-models', [CompletionResultType]::ParameterName, 'List all available models')
            [CompletionResult]::new('--list-roles', '--list-roles', [CompletionResultType]::ParameterName, 'List all available roles')
            [CompletionResult]::new('--list-sessions', '--list-sessions', [CompletionResultType]::ParameterName, 'List all available sessions')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-V', '-V', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
    })

    function Get-AichatValues($arg) {
        $(aichat $arg) -split '\n' | ForEach-Object { [CompletionResult]::new($_) }
    }

    if ($commandElements.Count -gt 1) {
        $offset=2
        if ($wordToComplete -eq "") {
            $offset=1
        }
        $flag = $commandElements[$commandElements.Count-$offset].ToString()
        if ($flag -eq "-m" -or $flag -eq "--model") {
            $completions = Get-AichatValues "--list-models"
        } elseif ($flag -eq "-r" -or $flag -eq "--role") {
            $completions = Get-AichatValues "--list-roles"
        } elseif ($flag -eq "-s" -or $flag -eq "--session") {
            $completions = Get-AichatValues "--list-sessions"
        } elseif ($flag -eq "-f" -or $flag -eq "--file") {
            $completions = @()
        }
    }

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
