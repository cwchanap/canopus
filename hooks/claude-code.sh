#!/usr/bin/env bash
# Claude Code notification hook for Canopus Inbox
#
# This script sends notifications to the Canopus inbox when Claude Code
# triggers certain events (permission prompts, idle prompts, etc.)
#
# Installation:
# 1. Save this script to ~/.config/claude-code/hooks/canopus-notify.sh
# 2. Make it executable: chmod +x ~/.config/claude-code/hooks/canopus-notify.sh
# 3. Configure Claude Code hooks in settings.json:
#    {
#      "hooks": {
#        "Notification": [
#          {
#            "matcher": "permission_prompt|idle_prompt",
#            "hooks": [
#              {
#                "type": "command",
#                "command": "~/.config/claude-code/hooks/canopus-notify.sh",
#                "timeout": 5
#              }
#            ]
#          }
#        ]
#      }
#    }

set -euo pipefail

# Get project name from environment or current directory
PROJECT_NAME="${CLAUDE_PROJECT_DIR:-$(pwd)}"
PROJECT_NAME="${PROJECT_NAME##*/}"  # Get basename

# Determine status and action based on notification type
NOTIFICATION_TYPE="${CLAUDE_NOTIFICATION_TYPE:-unknown}"

case "$NOTIFICATION_TYPE" in
    permission_prompt)
        STATUS="Waiting for permission approval"
        ACTION="Review and approve the permission request"
        ;;
    idle_prompt)
        STATUS="Waiting for user input"
        ACTION="Provide input or instructions to continue"
        ;;
    *)
        STATUS="Notification from Claude Code"
        ACTION="Check Claude Code for details"
        ;;
esac

# Send to Canopus inbox
canopus inbox add \
    --project "$PROJECT_NAME" \
    --status "$STATUS" \
    --action "$ACTION" \
    --agent claude-code

exit 0
