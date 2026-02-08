#!/bin/bash

WORKING="${WORKING:-$HOME/echokit_cc_sessions}"

mkdir -p $WORKING/$CLAUDE_SESSION_ID
cd $WORKING/$CLAUDE_SESSION_ID


HISTORY_FILE=$(echo "$PWD" | sed 's/[\/_]/-/g')

HISTORY_PATH="$HOME/.claude/projects/${HISTORY_FILE}/${CLAUDE_SESSION_ID}.jsonl"

echo "$HISTORY_PATH"

if [ -s "$HISTORY_PATH" ]; then
    echo "Resuming session: $CLAUDE_SESSION_ID"
    claude --resume "$CLAUDE_SESSION_ID"
else
    rm -rf "$HISTORY_PATH"
    echo "Starting new session: $CLAUDE_SESSION_ID"
    claude --session-id "$CLAUDE_SESSION_ID"
fi
