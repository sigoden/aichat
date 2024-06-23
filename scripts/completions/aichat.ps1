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
            [CompletionResult]::new('--save-session', '--save-session', [CompletionResultType]::ParameterName, 'Forces the session to be saved')
            [CompletionResult]::new('-a', '-a', [CompletionResultType]::ParameterName, 'Start a agent')
            [CompletionResult]::new('--agent', '--agent', [CompletionResultType]::ParameterName, 'Start a agent')
            [CompletionResult]::new('-R', '-R', [CompletionResultType]::ParameterName, 'Start a RAG')
            [CompletionResult]::new('--rag', '--rag', [CompletionResultType]::ParameterName, 'Start a RAG')
            [CompletionResult]::new('--serve', '--serve', [CompletionResultType]::ParameterName, 'Serve the LLM API and WebAPP')
            [CompletionResult]::new('-e', '-e', [CompletionResultType]::ParameterName, 'Execute commands in natural language')
            [CompletionResult]::new('--execute', '--execute', [CompletionResultType]::ParameterName, 'Execute commands in natural language')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Output code only')
            [CompletionResult]::new('--code', '--code', [CompletionResultType]::ParameterName, 'Output code only')
            [CompletionResult]::new('-f', '-f', [CompletionResultType]::ParameterName, 'Include files with the message')
            [CompletionResult]::new('--file', '--file', [CompletionResultType]::ParameterName, 'Include files with the message')
            [CompletionResult]::new('-S', '-S', [CompletionResultType]::ParameterName, 'Turn off stream mode')
            [CompletionResult]::new('--no-stream', '--no-stream', [CompletionResultType]::ParameterName, 'Turn off stream mode')
            [CompletionResult]::new('-w', '-w', [CompletionResultType]::ParameterName, 'Control text wrapping (no, auto, <max-width>)')
            [CompletionResult]::new('--wrap', '--wrap', [CompletionResultType]::ParameterName, 'Control text wrapping (no, auto, <max-width>)')
            [CompletionResult]::new('-H', '-H', [CompletionResultType]::ParameterName, 'Turn off syntax highlighting')
            [CompletionResult]::new('--no-highlight', '--no-highlight', [CompletionResultType]::ParameterName, 'Turn off syntax highlighting')
            [CompletionResult]::new('--light-theme', '--light-theme', [CompletionResultType]::ParameterName, 'Use light theme')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Display the message without sending it')
            [CompletionResult]::new('--info', '--info', [CompletionResultType]::ParameterName, 'Display information')
            [CompletionResult]::new('--list-models', '--list-models', [CompletionResultType]::ParameterName, 'List all available chat models')
            [CompletionResult]::new('--list-roles', '--list-roles', [CompletionResultType]::ParameterName, 'List all roles')
            [CompletionResult]::new('--list-sessions', '--list-sessions', [CompletionResultType]::ParameterName, 'List all sessions')
            [CompletionResult]::new('--list-agents', '--list-agents', [CompletionResultType]::ParameterName, 'List all agents')
            [CompletionResult]::new('--list-rags', '--list-rags', [CompletionResultType]::ParameterName, 'List all RAGs')
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
        dump-args $flag ($flag -eq "-R") > /tmp/file1
        if ($flag -ceq "-m" -or $flag -eq "--model") {
            $completions = Get-AichatValues "--list-models"
        } elseif ($flag -ceq "-r" -or $flag -eq "--role") {
            $completions = Get-AichatValues "--list-roles"
        } elseif ($flag -ceq "-s" -or $flag -eq "--session") {
            $completions = Get-AichatValues "--list-sessions"
        } elseif ($flag -ceq "-a" -or $flag -eq "--agent") {
            $completions = Get-AichatValues "--list-agents"
        } elseif ($flag -ceq "-R" -or $flag -eq "--rag") {
            $completions = Get-AichatValues "--list-rags"
        } elseif ($flag -ceq "-f" -or $flag -eq "--file") {
            $completions = @()
        }
    }

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
