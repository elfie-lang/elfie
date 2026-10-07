"""Adds the fields newer MCP clients require on a tools/list result, which the server's library omits."""
import json, subprocess, sys, threading

server = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)

def forward_input():
    for line in sys.stdin:
        server.stdin.write(line)
        server.stdin.flush()
    server.stdin.close()

threading.Thread(target=forward_input, daemon=True).start()
for line in server.stdout:
    try:
        message = json.loads(line)
        result = message.get("result") if isinstance(message, dict) else None
        if isinstance(result, dict) and isinstance(result.get("tools"), list):
            result.setdefault("ttlMs", 0)
            result.setdefault("cacheScope", "private")
            line = json.dumps(message) + "\n"
    except ValueError:
        pass
    sys.stdout.write(line)
    sys.stdout.flush()
sys.exit(server.wait())
