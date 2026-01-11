#!/usr/bin/env bash
# OpenCode AI CLI notification hook for Canopus Inbox
#
# This script sends notifications to the Canopus inbox when OpenCode
# completes tasks or needs user input.
#
# Installation:
# 1. Save this script to ~/.config/opencode/plugins/canopus-notify.sh
# 2. Make it executable: chmod +x ~/.config/opencode/plugins/canopus-notify.sh
# 3. Register as an OpenCode plugin by adding to ~/.config/opencode/config.yaml:
#    plugins:
#      - path: ~/.config/opencode/plugins/canopus-notify.sh
#        events:
#          - session_complete
#          - input_required
#
# Or create a TypeScript plugin (opencode supports 25+ event hooks):
#    See docs/hooks/opencode-plugin.ts for TypeScript example

set -euo pipefail

# Get project name from environment or current directory
PROJECT_NAME="${OPENCODE_PROJECT:-$(pwd)}"
PROJECT_NAME="${PROJECT_NAME##*/}"  # Get basename

# Determine status based on OpenCode event type
EVENT_TYPE="${OPENCODE_EVENT:-session_complete}"

case "$EVENT_TYPE" in
    session_complete)
        STATUS="OpenCode session completed"
        ACTION="Review generated code and commit changes"
        ;;
    input_required)
        STATUS="OpenCode waiting for input"
        ACTION="Provide required information to continue"
        ;;
    error)
        STATUS="OpenCode encountered an error"
        ACTION="Review error and retry or fix manually"
        ;;
    *)
        STATUS="OpenCode notification"
        ACTION="Check OpenCode for details"
        ;;
esac

# Send to Canopus inbox
canopus inbox add \
    --project "$PROJECT_NAME" \
    --status "$STATUS" \
    --action "$ACTION" \
    --agent opencode

exit 0
