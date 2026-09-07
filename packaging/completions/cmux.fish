# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_cmux_global_optspecs
	string join \n socket= json no-json v/verbose color= h/help V/version
end

function __fish_cmux_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_cmux_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_cmux_using_subcommand
	set -l cmd (__fish_cmux_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c cmux -n "__fish_cmux_needs_command" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_needs_command" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_needs_command" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_needs_command" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_needs_command" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_needs_command" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_needs_command" -s V -l version -d 'Print version'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "diff" -d 'Open a bounded patch or Git comparison in an agent-accessible diff surface'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "claude-teams" -d 'Launch Claude Code teams with teammate panes translated into native cmux splits'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "tmux-compat-internal" -d 'Private tmux compatibility endpoint used only by managed team launchers'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "project-run" -d 'Execute an explicitly requested project command after checking its inspected fingerprint'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "project-actions" -d 'Inspect resolved project actions and their source files without running them'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "hooks" -d 'Install and receive native agent session hooks'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "restore" -d 'Execute this terminal\'s saved manual resume command in the calling terminal'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "surface" -d 'Manage persistent terminal surface state'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "update" -d 'Update a self-managed cmux installation'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "ping" -d 'Ping the running cmux instance'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "identify" -d 'Show cmux instance identity (version, platform, pid)'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "capabilities" -d 'List supported socket commands'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "diagnostics" -d 'Show process resources and diagnostic logging health'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-workspaces" -d 'List all workspaces'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "current-workspace" -d 'Show the current workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "raw" -d 'Send an arbitrary JSON-RPC method'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "new-workspace" -d 'Create a new workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "ssh" -d 'Create a first-class remote workspace with SSH management'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "mosh" -d 'Create a remote workspace using Mosh for interactive terminals'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "mosh-tmux" -d 'Create a roaming Mosh terminal attached to a named remote tmux session'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "select-workspace" -d 'Select a workspace by ID'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "close-workspace" -d 'Close a workspace by ID'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "rename-workspace" -d 'Rename a workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "next-workspace" -d 'Switch to next workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "prev-workspace" -d 'Switch to previous workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "last-workspace" -d 'Switch to last active workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "reorder-workspace" -d 'Reorder a workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "reorder-workspaces" -d 'Reorder listed workspaces first, retaining the relative order of all others'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-workspace-groups" -d 'List persistent workspace groups and their members'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "create-workspace-group" -d 'Create an empty persistent workspace group'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "update-workspace-group" -d 'Update a workspace group\'s presentation or collapse state'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "assign-workspace-group" -d 'Assign workspaces to a group; omit --group to make them ungrouped'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "delete-workspace-group" -d 'Delete a group while retaining its workspaces'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-surfaces" -d 'List all surfaces'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "split" -d 'Split a surface'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "focus-surface" -d 'Focus a surface by ID'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "close-surface" -d 'Close a surface by ID'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "move-surface" -d 'Move a live surface tab into another pane in the same workspace'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "reorder-surface" -d 'Reorder a surface tab inside its current pane'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "drag-surface-to-split" -d 'Move a surface into a newly split pane next to a target pane'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "send-text" -d 'Send text to a surface'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "send-key" -d 'Send one literal character to a terminal surface'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "read-text" -d 'Read current terminal viewport text (up to 256 KiB)'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "read-scrollback" -d 'Capture recent terminal history as bounded VT text (up to 2,000 rows and 256 KiB)'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "health" -d 'Check native terminal availability and pane attention'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "refresh" -d 'Refresh a surface'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-panes" -d 'List all panes'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "focus-pane" -d 'Focus a pane'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "last-pane" -d 'Switch to last focused pane'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-windows" -d 'List all windows'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "current-window" -d 'Show current window info'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "layout" -d 'Show layout tree'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "type" -d 'Type text into the focused terminal'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "set-status" -d 'Set a keyed status in a workspace sidebar'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "report-meta-block" -d 'Publish a keyed multiline Markdown summary'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "clear-meta-block" -d 'Remove a keyed Markdown summary'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-meta-blocks" -d 'List retained Markdown summaries'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "clear-status" -d 'Clear one sidebar status key'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "ports" -d 'List attributed listening ports without changing workspace selection'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-status" -d 'List workspace status entries and progress'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "set-progress" -d 'Set determinate workspace progress from zero to one'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "clear-progress" -d 'Clear workspace progress'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "notify" -d 'Deliver a notification to a terminal without changing focus'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "notifications" -d 'Inspect, read, dismiss and navigate notification history'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "list-notifications" -d 'List notifications'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "clear-notification" -d 'Clear a notification'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "browser" -d 'Browser automation (agent primary interface)'
complete -c cmux -n "__fish_cmux_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l source -d 'Git source when no patch file is supplied' -r -f -a "unstaged\t''
staged\t''
branch\t''
last-turn\t''"
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l workspace -d 'Destination workspace UUID; defaults to the caller or selected workspace' -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l surface -d 'Place the viewer immediately to the right of this surface UUID' -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l session -d 'Select a provider session-specific last-turn baseline' -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l cwd -d 'Repository or child path used by Git sources' -r -F
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l base -d 'Explicit base ref for a branch comparison' -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l title -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l layout -r -f -a "unified\t''
split\t''"
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l font-size -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l unstaged
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l staged
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l branch
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l last-turn
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l focus -d 'Focus the new viewer after it opens'
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l no-focus -d 'Preserve the currently focused surface (the default)'
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand diff" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand diff" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand diff" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand claude-teams" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand claude-teams" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand claude-teams" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand claude-teams" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand claude-teams" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand claude-teams" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand tmux-compat-internal" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand tmux-compat-internal" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand tmux-compat-internal" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand tmux-compat-internal" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand tmux-compat-internal" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand tmux-compat-internal" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l fingerprint -r
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l confirm -d 'Confirm a reviewed action that requires an additional destructive decision'
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand project-run" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -l directory -r -F
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand project-actions" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "setup" -d 'Install supported hooks while preserving unrelated agent configuration'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "claude" -d 'Receive a Claude Code hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "codex" -d 'Receive a Codex lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "grok" -d 'Receive a Grok lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "gemini" -d 'Receive a Gemini lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "copilot" -d 'Receive a GitHub Copilot lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "codebuddy" -d 'Receive a CodeBuddy lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "factory" -d 'Receive a Factory Droid lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "qoder" -d 'Receive a Qoder lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "opencode" -d 'Receive an OpenCode plugin lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "cursor" -d 'Receive a Cursor Agent lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "pi" -d 'Receive a Pi coding agent extension lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "amp" -d 'Receive an Amp plugin lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "rovodev" -d 'Receive a Rovo Dev YAML hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and not __fish_seen_subcommand_from setup claude codex grok gemini copilot codebuddy factory qoder opencode cursor pi amp rovodev help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from setup" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from setup" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from setup" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from setup" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from setup" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from setup" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from claude" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codex" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from grok" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from gemini" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from copilot" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from codebuddy" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from factory" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from qoder" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from opencode" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from cursor" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from pi" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -f -a "session-start"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -f -a "session-end"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -f -a "notification"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from amp" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -f -a "prompt-submit"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -f -a "stop"
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from rovodev" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "setup" -d 'Install supported hooks while preserving unrelated agent configuration'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "claude" -d 'Receive a Claude Code hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "codex" -d 'Receive a Codex lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "grok" -d 'Receive a Grok lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "gemini" -d 'Receive a Gemini lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "copilot" -d 'Receive a GitHub Copilot lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "codebuddy" -d 'Receive a CodeBuddy lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "factory" -d 'Receive a Factory Droid lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "qoder" -d 'Receive a Qoder lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "opencode" -d 'Receive an OpenCode plugin lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "cursor" -d 'Receive a Cursor Agent lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "pi" -d 'Receive a Pi coding agent extension lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "amp" -d 'Receive an Amp plugin lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "rovodev" -d 'Receive a Rovo Dev YAML hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand hooks; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l surface -r
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l checkpoint -r
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l automatic -d 'Require a current application-signed approval before executing'
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand restore" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand restore" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand restore" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -f -a "resume" -d 'Register or inspect a saved resume command (does not execute it)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and not __fish_seen_subcommand_from resume help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -f -a "set"
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -f -a "show"
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -f -a "clear"
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from resume" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from help" -f -a "resume" -d 'Register or inspect a saved resume command (does not execute it)'
complete -c cmux -n "__fish_cmux_using_subcommand surface; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand update" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand update" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand update" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand update" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand update" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand update" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand ping" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand ping" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand ping" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand ping" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand ping" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand ping" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand identify" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand identify" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand identify" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand identify" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand identify" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand identify" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand capabilities" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand capabilities" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand capabilities" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand capabilities" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand capabilities" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand capabilities" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand diagnostics" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand diagnostics" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand diagnostics" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand diagnostics" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand diagnostics" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand diagnostics" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspaces" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-workspaces" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-workspaces" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspaces" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspaces" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspaces" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand current-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand current-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand current-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand current-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand current-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand current-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand raw" -l params -d 'JSON params string' -r
complete -c cmux -n "__fish_cmux_using_subcommand raw" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand raw" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand raw" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand raw" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand raw" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand raw" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -l name -d 'Display name (defaults to the selected folder name)' -r
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -l cwd -d 'Folder new terminals in this workspace start in' -r
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand new-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l transport -r -f -a "ssh\t''
mosh\t''"
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l name -r
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l directory -r
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand ssh" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -l name -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -l directory -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand mosh" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l session -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l name -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l directory -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand mosh-tmux" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand select-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand select-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand select-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand select-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand select-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand select-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand close-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand close-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand close-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand close-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand close-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand close-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand rename-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand rename-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand rename-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand rename-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand rename-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand rename-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand next-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand next-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand next-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand next-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand next-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand next-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand prev-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand prev-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand prev-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand prev-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand prev-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand prev-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand last-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand last-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand last-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand last-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand last-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand last-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspace" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspace" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspace" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspace" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspace" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspace" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -l order -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -l dry-run
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-workspaces" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspace-groups" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-workspace-groups" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-workspace-groups" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspace-groups" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspace-groups" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-workspace-groups" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand create-workspace-group" -l color -r
complete -c cmux -n "__fish_cmux_using_subcommand create-workspace-group" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand create-workspace-group" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand create-workspace-group" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand create-workspace-group" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand create-workspace-group" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l name -r
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l color -r
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l collapsed -r -f -a "true\t''
false\t''"
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l position -r
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l clear-color
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand update-workspace-group" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -l group -r
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -l workspaces -r
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand assign-workspace-group" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand delete-workspace-group" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand delete-workspace-group" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand delete-workspace-group" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand delete-workspace-group" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand delete-workspace-group" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand delete-workspace-group" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-surfaces" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-surfaces" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-surfaces" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-surfaces" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-surfaces" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-surfaces" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand split" -l direction -d 'Split direction: horizontal or vertical' -r
complete -c cmux -n "__fish_cmux_using_subcommand split" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand split" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand split" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand split" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand split" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand split" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand split" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand focus-surface" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand focus-surface" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand focus-surface" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand focus-surface" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand focus-surface" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand focus-surface" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand close-surface" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand close-surface" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand close-surface" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand close-surface" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand close-surface" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand close-surface" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l pane -d 'Destination pane reference (pane:N)' -r
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l workspace -d 'Destination workspace UUID; defaults to the pane owner or source workspace' -r
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l position -d 'Zero-based insertion position; defaults to the end' -r
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l no-focus -d 'Preserve current focus instead of selecting the moved surface'
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand move-surface" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-surface" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-surface" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand reorder-surface" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-surface" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-surface" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand reorder-surface" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -l pane -r
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -l direction -r -f -a "left\t''
right\t''
up\t''
down\t''"
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand drag-surface-to-split" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand send-text" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand send-key" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand read-text" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand read-scrollback" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand health" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand health" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand health" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand health" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand health" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand health" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand health" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -l id -d 'Target surface ID (default: focused)' -r
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand refresh" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-panes" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-panes" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-panes" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-panes" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-panes" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-panes" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand focus-pane" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand focus-pane" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand focus-pane" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand focus-pane" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand focus-pane" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand focus-pane" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand last-pane" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand last-pane" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand last-pane" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand last-pane" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand last-pane" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand last-pane" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-windows" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-windows" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-windows" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-windows" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-windows" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-windows" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand current-window" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand current-window" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand current-window" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand current-window" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand current-window" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand current-window" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand layout" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand layout" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand layout" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand layout" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand layout" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand layout" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand type" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand type" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand type" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand type" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand type" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand type" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l icon -r
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l color -r
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l priority -r
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l format -r -f -a "plain\t''
markdown\t''"
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l url -r
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand set-status" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -l priority -r
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand report-meta-block" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-meta-block" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-meta-blocks" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-status" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand ports" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand ports" -l surface -r
complete -c cmux -n "__fish_cmux_using_subcommand ports" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand ports" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand ports" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand ports" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand ports" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand ports" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-status" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -l label -r
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand set-progress" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-progress" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l title -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l subtitle -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l body -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l surface -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notify" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notify" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notify" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "list" -d 'List retained messages and read state'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "clear" -d 'Remove all messages, or messages in an explicit workspace/surface scope'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "mark-read" -d 'Mark a message, a workspace/surface scope, or all messages read without focus changes'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "dismiss" -d 'Remove one message or all previously read messages'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "open" -d 'Focus the exact terminal referenced by a message'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "jump-to-unread" -d 'Focus the most recent unread message\'s terminal'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and not __fish_seen_subcommand_from list clear mark-read dismiss open jump-to-unread help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from list" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from list" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from list" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from list" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from list" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l surface -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l caller -d 'Clear messages attributed to this calling terminal using native identity'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from clear" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l id -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l workspace -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l surface -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l all
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from mark-read" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -l id -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -l all-read
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from dismiss" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from open" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from open" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from open" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from open" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from open" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from open" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from jump-to-unread" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from jump-to-unread" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from jump-to-unread" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from jump-to-unread" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from jump-to-unread" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from jump-to-unread" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "list" -d 'List retained messages and read state'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "clear" -d 'Remove all messages, or messages in an explicit workspace/surface scope'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "mark-read" -d 'Mark a message, a workspace/surface scope, or all messages read without focus changes'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "dismiss" -d 'Remove one message or all previously read messages'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "open" -d 'Focus the exact terminal referenced by a message'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "jump-to-unread" -d 'Focus the most recent unread message\'s terminal'
complete -c cmux -n "__fish_cmux_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand list-notifications" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-notifications" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand list-notifications" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand list-notifications" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand list-notifications" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand list-notifications" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand clear-notification" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-notification" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand clear-notification" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand clear-notification" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-notification" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand clear-notification" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "open" -d 'Open a URL in the browser pane'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "list" -d 'List browser surfaces'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "close" -d 'Close browser surface(s)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "snapshot" -d 'Take a browser snapshot (accessibility tree / DOM text)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "click" -d 'Click an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "fill" -d 'Fill an input field (clears first, then types)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "type" -d 'Type text into an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "press" -d 'Press a key (e.g. "Enter", "Tab", "Escape")'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "hover" -d 'Hover over an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "scroll" -d 'Scroll the page'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "select" -d 'Select an option from a dropdown'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "eval" -d 'Evaluate JavaScript in the browser'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "wait" -d 'Wait for a condition'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "goto" -d 'Navigate to a URL'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "back" -d 'Go back in browser history'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "forward" -d 'Go forward in browser history'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "reload" -d 'Reload the current page'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "get-url" -d 'Get the current page URL'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "get-title" -d 'Get the current page title'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "get-text" -d 'Get text content of an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "get-html" -d 'Get HTML content of an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "screenshot" -d 'Take a browser screenshot (base64 PNG)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "stream-enable" -d 'Enable browser streaming'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "stream-disable" -d 'Disable browser streaming'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and not __fish_seen_subcommand_from open list close snapshot click fill type press hover scroll select eval wait goto back forward reload get-url get-title get-text get-html screenshot stream-enable stream-disable help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -l workspace -d 'Target workspace ID' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -l profile -d 'Chrome profile name or persistent profile directory used by agent-browser' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from open" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from list" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from list" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from list" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from list" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from list" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -l surface -d 'Surface reference (surface:N or UUID); closes all if omitted' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from close" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l max-depth -d 'Maximum depth' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l interactive -d 'Include interactive element annotations'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l compact -d 'Compact output'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from snapshot" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -l snapshot-after -d 'Take snapshot after action'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from click" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -l snapshot-after -d 'Take snapshot after action'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from fill" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from type" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from type" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from type" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from type" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from type" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from type" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from press" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from press" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from press" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from press" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from press" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from press" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from hover" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from hover" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from hover" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from hover" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from hover" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from hover" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -l amount -d 'Amount in pixels' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from scroll" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from select" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from select" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from select" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from select" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from select" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from select" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from eval" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from eval" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from eval" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from eval" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from eval" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from eval" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l selector -d 'CSS selector to wait for' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l text -d 'Text to wait for' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l url-contains -d 'URL substring to wait for' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l load-state -d 'Load state to wait for' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l function -d 'JavaScript function to wait for' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l timeout-ms -d 'Timeout in milliseconds' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from wait" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from goto" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from goto" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from goto" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from goto" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from goto" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from goto" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from back" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from back" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from back" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from back" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from back" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from back" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from forward" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from forward" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from forward" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from forward" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from forward" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from forward" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from reload" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from reload" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from reload" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from reload" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from reload" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from reload" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-url" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-url" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-url" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-url" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-url" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-url" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-title" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-title" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-title" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-title" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-title" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-title" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-text" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-text" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-text" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-text" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-text" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-text" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-html" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-html" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-html" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-html" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-html" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from get-html" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from screenshot" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from screenshot" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from screenshot" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from screenshot" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from screenshot" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from screenshot" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-enable" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-enable" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-enable" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-enable" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-enable" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-enable" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-disable" -l socket -d 'Path to the cmux socket (overrides discovery)' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-disable" -l color -d 'Color mode: always, never, auto' -r
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-disable" -l json -d 'Output raw JSON responses'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-disable" -l no-json -d 'Suppress JSON output for browser commands (browser defaults to JSON)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-disable" -s v -l verbose -d 'Verbose output (connection info to stderr)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from stream-disable" -s h -l help -d 'Print help'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "open" -d 'Open a URL in the browser pane'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "list" -d 'List browser surfaces'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "close" -d 'Close browser surface(s)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "snapshot" -d 'Take a browser snapshot (accessibility tree / DOM text)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "click" -d 'Click an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "fill" -d 'Fill an input field (clears first, then types)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "type" -d 'Type text into an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "press" -d 'Press a key (e.g. "Enter", "Tab", "Escape")'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "hover" -d 'Hover over an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "scroll" -d 'Scroll the page'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "select" -d 'Select an option from a dropdown'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "eval" -d 'Evaluate JavaScript in the browser'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "wait" -d 'Wait for a condition'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "goto" -d 'Navigate to a URL'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "back" -d 'Go back in browser history'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "forward" -d 'Go forward in browser history'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "reload" -d 'Reload the current page'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "get-url" -d 'Get the current page URL'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "get-title" -d 'Get the current page title'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "get-text" -d 'Get text content of an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "get-html" -d 'Get HTML content of an element'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "screenshot" -d 'Take a browser screenshot (base64 PNG)'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "stream-enable" -d 'Enable browser streaming'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "stream-disable" -d 'Disable browser streaming'
complete -c cmux -n "__fish_cmux_using_subcommand browser; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "diff" -d 'Open a bounded patch or Git comparison in an agent-accessible diff surface'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "claude-teams" -d 'Launch Claude Code teams with teammate panes translated into native cmux splits'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "tmux-compat-internal" -d 'Private tmux compatibility endpoint used only by managed team launchers'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "project-run" -d 'Execute an explicitly requested project command after checking its inspected fingerprint'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "project-actions" -d 'Inspect resolved project actions and their source files without running them'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "hooks" -d 'Install and receive native agent session hooks'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "restore" -d 'Execute this terminal\'s saved manual resume command in the calling terminal'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "surface" -d 'Manage persistent terminal surface state'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "update" -d 'Update a self-managed cmux installation'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "ping" -d 'Ping the running cmux instance'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "identify" -d 'Show cmux instance identity (version, platform, pid)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "capabilities" -d 'List supported socket commands'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "diagnostics" -d 'Show process resources and diagnostic logging health'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-workspaces" -d 'List all workspaces'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "current-workspace" -d 'Show the current workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "raw" -d 'Send an arbitrary JSON-RPC method'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "new-workspace" -d 'Create a new workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "ssh" -d 'Create a first-class remote workspace with SSH management'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "mosh" -d 'Create a remote workspace using Mosh for interactive terminals'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "mosh-tmux" -d 'Create a roaming Mosh terminal attached to a named remote tmux session'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "select-workspace" -d 'Select a workspace by ID'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "close-workspace" -d 'Close a workspace by ID'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "rename-workspace" -d 'Rename a workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "next-workspace" -d 'Switch to next workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "prev-workspace" -d 'Switch to previous workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "last-workspace" -d 'Switch to last active workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "reorder-workspace" -d 'Reorder a workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "reorder-workspaces" -d 'Reorder listed workspaces first, retaining the relative order of all others'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-workspace-groups" -d 'List persistent workspace groups and their members'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "create-workspace-group" -d 'Create an empty persistent workspace group'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "update-workspace-group" -d 'Update a workspace group\'s presentation or collapse state'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "assign-workspace-group" -d 'Assign workspaces to a group; omit --group to make them ungrouped'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "delete-workspace-group" -d 'Delete a group while retaining its workspaces'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-surfaces" -d 'List all surfaces'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "split" -d 'Split a surface'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "focus-surface" -d 'Focus a surface by ID'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "close-surface" -d 'Close a surface by ID'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "move-surface" -d 'Move a live surface tab into another pane in the same workspace'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "reorder-surface" -d 'Reorder a surface tab inside its current pane'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "drag-surface-to-split" -d 'Move a surface into a newly split pane next to a target pane'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "send-text" -d 'Send text to a surface'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "send-key" -d 'Send one literal character to a terminal surface'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "read-text" -d 'Read current terminal viewport text (up to 256 KiB)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "read-scrollback" -d 'Capture recent terminal history as bounded VT text (up to 2,000 rows and 256 KiB)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "health" -d 'Check native terminal availability and pane attention'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "refresh" -d 'Refresh a surface'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-panes" -d 'List all panes'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "focus-pane" -d 'Focus a pane'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "last-pane" -d 'Switch to last focused pane'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-windows" -d 'List all windows'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "current-window" -d 'Show current window info'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "layout" -d 'Show layout tree'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "type" -d 'Type text into the focused terminal'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "set-status" -d 'Set a keyed status in a workspace sidebar'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "report-meta-block" -d 'Publish a keyed multiline Markdown summary'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "clear-meta-block" -d 'Remove a keyed Markdown summary'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-meta-blocks" -d 'List retained Markdown summaries'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "clear-status" -d 'Clear one sidebar status key'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "ports" -d 'List attributed listening ports without changing workspace selection'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-status" -d 'List workspace status entries and progress'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "set-progress" -d 'Set determinate workspace progress from zero to one'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "clear-progress" -d 'Clear workspace progress'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "notify" -d 'Deliver a notification to a terminal without changing focus'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "notifications" -d 'Inspect, read, dismiss and navigate notification history'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "list-notifications" -d 'List notifications'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "clear-notification" -d 'Clear a notification'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "browser" -d 'Browser automation (agent primary interface)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and not __fish_seen_subcommand_from diff claude-teams tmux-compat-internal project-run project-actions hooks restore surface update ping identify capabilities diagnostics list-workspaces current-workspace raw new-workspace ssh mosh mosh-tmux select-workspace close-workspace rename-workspace next-workspace prev-workspace last-workspace reorder-workspace reorder-workspaces list-workspace-groups create-workspace-group update-workspace-group assign-workspace-group delete-workspace-group list-surfaces split focus-surface close-surface move-surface reorder-surface drag-surface-to-split send-text send-key read-text read-scrollback health refresh list-panes focus-pane last-pane list-windows current-window layout type set-status report-meta-block clear-meta-block list-meta-blocks clear-status ports list-status set-progress clear-progress notify notifications list-notifications clear-notification browser help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "setup" -d 'Install supported hooks while preserving unrelated agent configuration'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "claude" -d 'Receive a Claude Code hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "codex" -d 'Receive a Codex lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "grok" -d 'Receive a Grok lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "gemini" -d 'Receive a Gemini lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "copilot" -d 'Receive a GitHub Copilot lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "codebuddy" -d 'Receive a CodeBuddy lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "factory" -d 'Receive a Factory Droid lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "qoder" -d 'Receive a Qoder lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "opencode" -d 'Receive an OpenCode plugin lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "cursor" -d 'Receive a Cursor Agent lifecycle hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "pi" -d 'Receive a Pi coding agent extension lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "amp" -d 'Receive an Amp plugin lifecycle payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from hooks" -f -a "rovodev" -d 'Receive a Rovo Dev YAML hook payload on stdin'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from surface" -f -a "resume" -d 'Register or inspect a saved resume command (does not execute it)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "list" -d 'List retained messages and read state'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "clear" -d 'Remove all messages, or messages in an explicit workspace/surface scope'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "mark-read" -d 'Mark a message, a workspace/surface scope, or all messages read without focus changes'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "dismiss" -d 'Remove one message or all previously read messages'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "open" -d 'Focus the exact terminal referenced by a message'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "jump-to-unread" -d 'Focus the most recent unread message\'s terminal'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "open" -d 'Open a URL in the browser pane'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "list" -d 'List browser surfaces'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "close" -d 'Close browser surface(s)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "snapshot" -d 'Take a browser snapshot (accessibility tree / DOM text)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "click" -d 'Click an element'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "fill" -d 'Fill an input field (clears first, then types)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "type" -d 'Type text into an element'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "press" -d 'Press a key (e.g. "Enter", "Tab", "Escape")'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "hover" -d 'Hover over an element'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "scroll" -d 'Scroll the page'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "select" -d 'Select an option from a dropdown'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "eval" -d 'Evaluate JavaScript in the browser'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "wait" -d 'Wait for a condition'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "goto" -d 'Navigate to a URL'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "back" -d 'Go back in browser history'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "forward" -d 'Go forward in browser history'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "reload" -d 'Reload the current page'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "get-url" -d 'Get the current page URL'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "get-title" -d 'Get the current page title'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "get-text" -d 'Get text content of an element'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "get-html" -d 'Get HTML content of an element'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "screenshot" -d 'Take a browser screenshot (base64 PNG)'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "stream-enable" -d 'Enable browser streaming'
complete -c cmux -n "__fish_cmux_using_subcommand help; and __fish_seen_subcommand_from browser" -f -a "stream-disable" -d 'Disable browser streaming'
