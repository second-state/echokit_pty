#!/bin/bash

ECHOKIT_WORKING_PATH="${ECHOKIT_WORKING_PATH:-$HOME/echokit_cc_sessions}"

mkdir -p $ECHOKIT_WORKING_PATH/$CLAUDE_SESSION_ID
cd $ECHOKIT_WORKING_PATH/$CLAUDE_SESSION_ID


HISTORY_FILE=$(echo "$PWD" | sed 's/[\/_]/-/g')

HISTORY_PATH="$HOME/.claude/projects/${HISTORY_FILE}/${CLAUDE_SESSION_ID}.jsonl"

# Pre-create the history directory so linemux can attach before Claude writes to it
mkdir -p "$(dirname "$HISTORY_PATH")"

echo "$HISTORY_PATH"

if [ -s "$HISTORY_PATH" ]; then
    echo "Resuming session: $CLAUDE_SESSION_ID"
    claude --resume "$CLAUDE_SESSION_ID"
else
    rm -f "$HISTORY_PATH"
    echo "Starting new session: $CLAUDE_SESSION_ID"
    claude --session-id "$CLAUDE_SESSION_ID"
fi
