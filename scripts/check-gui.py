"""Compare what the desktop app shows with `onyx health --json`.

This is the exit test of P5. It starts the app with the MCP server of Slint,
presses the tab of each drive, reads the text that the window shows, and
compares it with what the command line prints for the same drive. It needs
an app that is built with the MCP feature:

    SLINT_EMIT_DEBUG_INFO=1 cargo build -p onyx-gui -p onyx --features slint/mcp
    python3 scripts/check-gui.py target/debug/OnyxDiskInfo target/debug/onyx

Set --headless for a computer with no display, such as a CI runner. Run it
with the same rights for both tools: with sudo on Linux, so that both read
the drive the same way.

Values that move between the two reads, the temperature and the hours, may
differ by a little. The script reads only. It writes nothing to a drive.
"""

import argparse
import base64
import http.client
import json
import os
import socket
import subprocess
import sys
import time

PORT = 9316
TEMPERATURE_SLACK = 3
HOURS_SLACK = 1


class Mcp:
    """A small client for the MCP server of a Slint application."""

    def __init__(self, port):
        self.port = port
        self.next_id = 1

    def call(self, tool, arguments=None):
        body = json.dumps({
            "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments or {}},
        })
        self.next_id += 1
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=30)
        try:
            connection.request("POST", "/mcp", body, {
                "Content-Type": "application/json",
                "Accept": "application/json, text/event-stream",
                "Connection": "close",
            })
            text = connection.getresponse().read().decode()
        finally:
            connection.close()
        if not text.lstrip().startswith("{"):
            text = "".join(line[5:] for line in text.splitlines() if line.startswith("data:"))
        answer = json.loads(text)
        result = answer.get("result", {})
        if "error" in answer or result.get("isError"):
            raise RuntimeError(f"{tool}: {answer.get('error') or result.get('content')}")
        return result.get("content", [])

    def data(self, tool, arguments=None):
        for item in self.call(tool, arguments):
            if item.get("type") == "text":
                return json.loads(item["text"])
        return None

    def window(self):
        return self.data("list_windows")["windowHandles"][0]

    def elements(self, window, role):
        """Each (handle, label) of an accessible role, in the order of the tree."""
        props = self.data("get_window_properties", {"windowHandle": window})
        found = self.data("query_element_descendants", {
            "elementHandle": props["rootElementHandle"],
            "findAll": True,
            "queryStack": [{"matchDescendants": True}, {"matchElementAccessibleRole": role}],
        })
        handles = found.get("elementHandles", []) if isinstance(found, dict) else found
        out, seen = [], set()
        for handle in handles:
            if handle["index"] in seen:
                continue
            seen.add(handle["index"])
            element = self.data("get_element_properties", {"elementHandle": handle})
            out.append((handle, element.get("accessibleLabel") or ""))
        return out

    def texts(self, window):
        return [label for _, label in self.elements(window, "Text")]

    def press(self, handle):
        self.call("invoke_accessibility_action", {"elementHandle": handle, "action": "Default_"})

    def screenshot(self, window, path):
        for item in self.call("take_screenshot", {"windowHandle": window}):
            if item.get("type") == "image":
                with open(path, "wb") as f:
                    f.write(base64.b64decode(item["data"]))


def wait_for_port(port, process, seconds=60):
    end = time.time() + seconds
    while time.time() < end:
        if process.poll() is not None:
            sys.exit(f"the app stopped with code {process.returncode}")
        try:
            socket.create_connection(("127.0.0.1", port), timeout=1).close()
            return
        except OSError:
            time.sleep(0.2)
    sys.exit("the MCP server of the app did not start")


def after(texts, label):
    """The text that follows a label in the window, such as a value in the grid."""
    try:
        return texts[texts.index(label) + 1]
    except (ValueError, IndexError):
        return None


def number_in(text):
    """The first whole number in a text such as `41 C` or `1,234 hours`."""
    if not text:
        return None
    digits = text.split(" ")[0].replace(",", "")
    return int(digits) if digits.lstrip("-").isdigit() else None


def wait_until_read(mcp, window, seconds=120):
    """Wait until the app has read every drive: the button says Refresh."""
    end = time.time() + seconds
    while time.time() < end:
        if any(label == "Refresh" for _, label in mcp.elements(window, "Button")):
            return
        time.sleep(0.5)
    sys.exit("the app did not finish reading the drives")


def compare(number, texts, health, why):
    fails = []
    shown = after(texts, "Health Status")
    if health is None:
        # The command line read nothing, so the app must show no verdict
        # either, and the reason in its place.
        if shown not in ("Locked", "Unknown"):
            fails.append(f"the app shows {shown!r}, and onyx read nothing: {why}")
        return fails
    verdict = health["health"]["verdict"]
    if shown != verdict:
        fails.append(f"verdict {shown!r} and not {verdict!r}")
    # A tab shows the verdict too, so the confidence is found by its words
    # and not by its place after the verdict.
    confidence = next((t for t in texts if t.startswith("Confidence: ")), None)
    if confidence != f"Confidence: {health['confidence']}":
        fails.append(f"{confidence!r} and not the confidence {health['confidence']!r}")
    for label, key in [("Model", "model"), ("Serial", "serial"), ("Firmware", "firmware")]:
        if health.get(key) and after(texts, label) != health[key]:
            fails.append(f"{label} {after(texts, label)!r} and not {health[key]!r}")
    temperature = number_in(after(texts, "Temperature"))
    if health.get("temperature_celsius") is not None:
        if temperature is None or abs(temperature - health["temperature_celsius"]) > TEMPERATURE_SLACK:
            fails.append(f"temperature {temperature} and not {health['temperature_celsius']}")
    hours = number_in(after(texts, "Power On Hours"))
    if health.get("power_on_hours") is not None:
        if hours is None or abs(hours - health["power_on_hours"]) > HOURS_SLACK:
            fails.append(f"power on hours {hours} and not {health['power_on_hours']}")
    return fails


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("app")
    parser.add_argument("onyx")
    parser.add_argument("--headless", action="store_true")
    parser.add_argument("--shots", help="a directory for a screenshot of each drive")
    args = parser.parse_args()

    def listed():
        return json.loads(subprocess.run(
            [args.onyx, "list", "--json"], check=True, capture_output=True, text=True,
        ).stdout)["drives"]

    drives = listed()

    env = dict(os.environ, SLINT_MCP_PORT=str(PORT))
    if args.headless:
        env["SLINT_BACKEND"] = "headless"
    app = subprocess.Popen([args.app], env=env)
    failures = []
    try:
        wait_for_port(PORT, app)
        mcp = Mcp(PORT)
        window = mcp.window()
        wait_until_read(mcp, window)
        tabs = [handle for handle, _ in mcp.elements(window, "Tab")]
        # A host can attach a drive while the test runs: the macOS runner
        # attaches disk images in the background, and once went from 5 to 15
        # between the list and the app. So when the two differ, the app reads
        # again, and so does onyx.
        for _ in range(3):
            if len(tabs) == len(drives):
                break
            print(f"the app shows {len(tabs)} drives, and onyx lists {len(drives)}: reading again")
            refresh = next(h for h, label in mcp.elements(window, "Button") if label == "Refresh")
            mcp.press(refresh)
            time.sleep(1)
            wait_until_read(mcp, window)
            drives = listed()
            tabs = [handle for handle, _ in mcp.elements(window, "Tab")]
        if len(tabs) != len(drives):
            failures.append(f"the app shows {len(tabs)} drives, and onyx lists {len(drives)}")
        for drive, tab in zip(drives, tabs):
            mcp.press(tab)
            time.sleep(0.3)
            texts = mcp.texts(window)
            done = subprocess.run(
                [args.onyx, "health", str(drive["number"]), "--json"],
                capture_output=True, text=True,
            )
            health = json.loads(done.stdout) if done.returncode == 0 else None
            fails = compare(drive["number"], texts, health, done.stderr.strip())
            label = f"{drive['number']} ({drive['id']}, {drive['name']})"
            shown = after(texts, "Health Status")
            print(f"{label}: the app shows {shown}, {after(texts, 'Temperature')}"
                  + ("" if fails else ", and it agrees with onyx"))
            failures += [f"{label}: {f}" for f in fails]
            if args.shots:
                os.makedirs(args.shots, exist_ok=True)
                mcp.screenshot(window, os.path.join(args.shots, f"drive-{drive['number']}.png"))
    finally:
        app.terminate()
        app.wait(timeout=10)

    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print(f"{len(drives)} drives: the app shows what onyx prints.")


if __name__ == "__main__":
    main()
