#!/usr/bin/env bash
# OpenAI Codex CLI notification hook for Canopus Inbox
#
# This script sends notifications to the Canopus inbox when Codex CLI
# completes a task or needs user input.
#
# Installation:
# 1. Save this script to ~/.config/codex/hooks/canopus-notify.sh
# 2. Make it executable: chmod +x ~/.config/codex/hooks/canopus-notify.sh
# 3. Configure Codex CLI in config.toml:
#    notify = ["~/.config/codex/hooks/canopus-notify.sh"]
#
# Or use as a wrapper:
#    alias codex='command codex "$@"; ~/.config/codex/hooks/canopus-notify.sh'

set -euo pipefail

# Get project name from environment or current directory
PROJECT_NAME="${CODEX_PROJECT:-$(pwd)}"
PROJECT_NAME="${PROJECT_NAME##*/}"  # Get basename

# Determine status based on Codex event type
EVENT_TYPE="${CODEX_EVENT_TYPE:-agent-turn-complete}"

case "$EVENT_TYPE" in
    agent-turn-complete)
        STATUS="Codex task completed"
        ACTION="Review generated code and provide feedback"
        ;;
    approval-requested)
        STATUS="Codex waiting for approval"
        ACTION="Approve or reject the pending action"
        ;;
    *)
        STATUS="Codex notification"
        ACTION="Check Codex CLI for details"
        ;;
esac

# Send to Canopus inbox
canopus inbox add \
    --project "$PROJECT_NAME" \
    --status "$STATUS" \
    --action "$ACTION" \
    --agent codex

exit 0
