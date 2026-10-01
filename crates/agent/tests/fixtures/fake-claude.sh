#!/bin/sh
# A stand-in for `claude` in the process tests of the driver
# (crates/agent/src/provider/claude/process_tests.rs). FAKE_CLAUDE picks what it does. The
# driver's flags are ignored.

case "$FAKE_CLAUDE" in
replay)
    # A recorded run: reads the request to start and the message, writes what the CLI wrote,
    # then waits for stdin to close, as the CLI does.
    read -r request
    read -r message
    sed -n 's/^{"received": \(.*\)}$/\1/p' "$FAKE_FIXTURE"
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
    # Reads nothing for a second, then answers with the length of the message line.
    sleep 1
    read -r request
    read -r message
    printf '{"type": "assistant", "message": {"content": [{"type": "text", "text": "%s"}]}}\n' "${#message}"
    cat >/dev/null
    ;;
esac
