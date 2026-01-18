# AI Agent Hooks for Canopus Inbox

This directory contains example hook scripts for integrating various AI coding agents with the Canopus inbox system.

## Overview

The Canopus inbox provides a unified queue for notifications from AI coding agents. When agents need user input or complete tasks, they can send notifications to the inbox via the `canopus inbox add` CLI command.

## Supported Agents

| Agent       | Hook Type               | Configuration                   |
| ----------- | ----------------------- | ------------------------------- |
| Claude Code | Shell script            | `~/.claude/settings.json` hooks |
| Codex CLI   | Shell script            | `config.toml` notify setting    |
| Windsurf    | JSON config             | `.windsurf/cascade-hooks.json`  |
| `OpenCode`  | Shell/TypeScript plugin | `~/.config/opencode/plugins/`   |

## Quick Start

### 1. Test the inbox CLI

```bash
# Add a test notification
canopus inbox add \
  --project "my-project" \
  --status "Test notification" \
  --action "Verify inbox is working" \
  --agent claude-code

# List inbox items
canopus inbox list

# Check for new items (shows desktop notification)
canopus inbox check
```

### 2. Install agent-specific hooks

See the individual hook files in this directory:

- `claude-code.sh` - Claude Code notification hook
- `codex.sh` - `OpenAI` Codex CLI hook
- `windsurf.json` - Windsurf Cascade Hooks config
- `opencode.sh` - `OpenCode` AI CLI hook

## CLI Commands

```bash
# List all inbox items
canopus inbox list

# List only unread items
canopus inbox list --status unread

# List items from a specific agent
canopus inbox list --agent claude-code

# View a specific item (marks as read)
canopus inbox view <item-id>

# Add a new item (used by hooks)
canopus inbox add \
  --project "project-name" \
  --status "Status summary" \
  --action "Action required" \
  --agent claude-code

# Mark items as read
canopus inbox read <item-id> [<item-id>...]

# Dismiss items
canopus inbox dismiss <item-id> [<item-id>...]

# Check for new items with notification
canopus inbox check

# Clean up old items (default: 7 days)
canopus inbox cleanup --days 7
```

## Agent-Specific Setup

### Claude Code

1. Copy `claude-code.sh` to `~/.config/claude-code/hooks/canopus-notify.sh`
2. Make executable: `chmod +x ~/.config/claude-code/hooks/canopus-notify.sh`
3. Add to Claude Code settings:

```json
{
  "hooks": {
    "Notification": [
      {
        "matcher": "permission_prompt|idle_prompt",
        "hooks": [
          {
            "type": "command",
            "command": "~/.config/claude-code/hooks/canopus-notify.sh",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

### Codex CLI

1. Copy `codex.sh` to `~/.config/codex/hooks/canopus-notify.sh`
2. Make executable: `chmod +x ~/.config/codex/hooks/canopus-notify.sh`
3. Add to Codex config.toml:

```toml
notify = ["~/.config/codex/hooks/canopus-notify.sh"]
```

### Windsurf

1. Copy `windsurf.json` to your workspace: `.windsurf/cascade-hooks.json`
2. Restart Windsurf to apply hooks

### `OpenCode`

1. Copy `opencode.sh` to `~/.config/opencode/plugins/canopus-notify.sh`
2. Make executable: `chmod +x ~/.config/opencode/plugins/canopus-notify.sh`
3. Add to `OpenCode` config.yaml:

```yaml
plugins:
  - path: ~/.config/opencode/plugins/canopus-notify.sh
    events:
      - session_complete
      - input_required
```

## Desktop Notifications

The inbox automatically sends desktop notifications when:

- A new item is added via `canopus inbox add`
- Running `canopus inbox check` finds unread items

Notifications use:

- **macOS**: Native Notification Center via `osascript`
- **Linux**: `notify-send` (libnotify)
- **Windows**: Windows Toast notifications

## Automatic Cleanup

Items are automatically cleaned up after 7 days by default. You can manually run cleanup:

```bash
canopus inbox cleanup --days 7
```

Or configure a cron job for periodic cleanup:

```bash
# Run cleanup daily at midnight
0 0 * * * canopus inbox cleanup --days 7
```

## Polling for New Items

You can set up periodic checks for new items:

```bash
# Check every 5 minutes
*/5 * * * * canopus inbox check --quiet

# Or add to shell prompt (check on each command)
PROMPT_COMMAND='canopus inbox list --status unread --count 2>/dev/null'
```

## Troubleshooting

### Notifications not appearing

1. Check if `canopus` is in PATH
2. Verify notification permissions on your OS
3. Test manually: `canopus inbox add --project test --status "Test" --action "Test" --agent other`

### Hook not triggering

1. Verify the hook script is executable: `chmod +x <script>`
2. Check agent-specific configuration
3. Test the hook script manually

### Database issues

The inbox database is stored at `~/.canopus/inbox.db`. If you encounter issues:

```bash
# Check database location
ls -la ~/.canopus/inbox.db

# Reset database (warning: deletes all items)
rm ~/.canopus/inbox.db
```
