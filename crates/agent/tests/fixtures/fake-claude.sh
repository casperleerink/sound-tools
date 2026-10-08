#!/bin/sh
# A stand-in for `claude` in the process tests of the driver
# (crates/agent/src/provider/claude/process_tests.rs). FAKE_CLAUDE picks what it does. The
# driver's flags are ignored.

if [ "$1" = auth ]; then
    # `auth status`, `auth login` and `auth logout`. The file FAKE_ACCOUNT says signed in.
    case "$2" in
    status)
        if [ "$FAKE_CLAUDE" = broken ]; then
            echo "Error: the config is damaged" >&2
            exit 2
        fi
        if [ -f "$FAKE_ACCOUNT" ]; then
            echo '{"loggedIn": true, "authMethod": "claude.ai", "email": "composer@example.com", "subscriptionType": "pro"}'
        else
            echo '{"loggedIn": false, "authMethod": "none"}'
            exit 1
        fi
        ;;
    login)
        echo "If the browser didn't open, visit: https://example.com/oauth"
        case "$FAKE_CLAUDE" in
        login) touch "$FAKE_ACCOUNT" ;;
        login_fails)
            echo "OAuth login failed: access denied" >&2
            exit 1
            ;;
        login_waits)
            # Waits for the browser forever. Keeps its pid in FAKE_OUTPUT.
            echo $$ >"$FAKE_OUTPUT"
            exec sleep 1000
            ;;
        esac
        ;;
    logout) rm -f "$FAKE_ACCOUNT" ;;
    esac
    exit 0
fi

case "$FAKE_CLAUDE" in
replay)
    # A recorded run: reads the request to start and the message, writes what the CLI wrote,
    # then waits for stdin to close, as the CLI does.
    read -r request
    read -r message
    sed -n 's/^{"received": \(.*\)}$/\1/p' "$FAKE_FIXTURE"
    cat >/dev/null
    ;;
models)
    # Answers the request to start, and nothing more: no message comes.
    read -r request
    sed -n 's/^{"received": \({"type": "control_response".*\)}$/\1/p' "$FAKE_FIXTURE"
    cat >/dev/null
    ;;
malformed)
    # A request and a result of known types that do not parse. Keeps the answer to the
    # request in FAKE_OUTPUT.
    read -r request
    read -r message
    echo '{"type": "control_request", "request_id": "broken", "request": 5}'
    read -r answer
    echo "$answer" >"$FAKE_OUTPUT"
    echo '{"type": "result", "subtype": 5, "request_id": 7}'
    cat >/dev/null
    ;;
children)
    # Starts a child of its own, keeps its pid in FAKE_OUTPUT, and never ends.
    sleep 1000 &
    echo $! >"$FAKE_OUTPUT"
    wait
    ;;
slow_reader)
    # Reads nothing until FAKE_OUTPUT exists, then answers with the length of the message
    # line.
    while [ ! -e "$FAKE_OUTPUT" ]; do sleep 0.02; done
    read -r request
    read -r message
    printf '{"type": "assistant", "message": {"content": [{"type": "text", "text": "%s"}]}}\n' "${#message}"
    cat >/dev/null
    ;;
esac
