using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'aichat' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $models = aichat --list-models
    $roles = aichat --list-roles
    $sessions = aichat --list-sessions

    $optionsWithArgs = @{
        '-m' = $models
        '--model' = $models
        '-r' = $roles
        '--role' = $roles
        '-s' = $sessions
        '--session' = $sessions
        '-f' = @()  # Placeholder for file completion
        '--file' = @()  # Placeholder for file completion
        '-w' = @('no', 'auto')
        '--wrap' = @('no', 'auto')
    }

    $optionsWithoutArgs = @(
        '-H', '--no-highlight',
        '-S', '--no-stream',
        '-e', '--execute',
        '-c', '--code',
        '--save-session',
        '--light-theme',
        '--dry-run',
        '--info',
        '--list-models',
        '--list-roles',
        '--list-sessions',
        '-h', '--help',
        '-V', '--version'
    )

    $commandElements = $commandAst.CommandElements
    $previousElement = $commandElements[$commandElements.Count - 2]

    if ($optionsWithArgs.ContainsKey($previousElement.Value)) {
        $completions = $optionsWithArgs[$previousElement.Value] |
            Where-Object { $_ -like "$wordToComplete*" } |
            ForEach-Object { [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterValue, $_) }
    }
    elseif ($wordToComplete -like '-*') {
        $allOptions = $optionsWithArgs.Keys + $optionsWithoutArgs
        $completions = $allOptions |
            Where-Object { $_ -like "$wordToComplete*" } |
            ForEach-Object { [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterName, $_) }
    }
    else {
        $completions = @()
    }

    if ($previousElement.Value -eq '-f' -or $previousElement.Value -eq '--file') {
        Register-ArgumentCompleter -CommandName 'aichat' -ParameterName 'file' -ScriptBlock {
            param($commandName, $parameterName, $wordToComplete, $commandAst, $fakeBoundParameters)
            $completions = Get-ChildItem -Path . -Name |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object { [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterValue, $_) }
            $completions
        }
    }

    $completions |
        Sort-Object -Property ListItemText
}
