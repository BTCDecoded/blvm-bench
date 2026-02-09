#!/bin/bash
# Safety check before running potentially destructive commands
# This should be called before ANY rm/mv/delete operation

COMMAND="$*"

# Check if command contains destructive operations
if echo "$COMMAND" | grep -qE "\b(rm|rmdir|delete|unlink|mv|remove)\b"; then
    # Check if it targets protected paths
    if echo "$COMMAND" | grep -qE "(blockchain|chunk.*\.zst|/run/media/acolyte/Extra/blockchain)"; then
        echo "❌❌❌ BLOCKED: Destructive command targeting chunks/blockchain!" >&2
        echo "Command: $COMMAND" >&2
        echo "" >&2
        echo "⚠️  CHUNKS ARE PROTECTED - DO NOT DELETE/MODIFY" >&2
        echo "Read SAFETY_RULES.md before proceeding" >&2
        exit 1
    fi
fi

exit 0























