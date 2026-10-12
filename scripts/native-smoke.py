#!/usr/bin/env python3
"""Launch the packaged native GUI and verify its shared IPC engine."""
import json, os, pathlib, subprocess, tempfile, time
root = pathlib.Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory(prefix="qrtx-native-") as directory:
    state = pathlib.Path(directory) / "state"
    environment = dict(os.environ, QRTX_STATE=str(state))
    with (pathlib.Path(directory) / "desktop.log").open("w+") as log:
        app = subprocess.Popen([str(root / "target/release/qrtx-desktop")], env=environment, stdout=log, stderr=log)
        try:
            for _ in range(150):
                if app.poll() is not None:
                    log.seek(0)
                    raise RuntimeError(log.read())
                if (state / "engine.sock").exists():
                    break
                time.sleep(.1)
            cli = [str(root / "target/debug/qrtransceiver"), "--state", str(state)]
            status = json.loads(subprocess.check_output(cli + ["status"], timeout=10))
            devices = json.loads(subprocess.check_output(cli + ["devices"], timeout=10))
            assert status["version"] == "0.1.0"
            assert status["buildHash"] != "unknown"
            assert isinstance(devices["displays"], list)
            time.sleep(2)
            assert app.poll() is None
            print(json.dumps({"ok":True, "checks":["native GTK/WebKit startup","desktop IPC engine","display enumeration"],"buildHash":status["buildHash"],"displayCount":len(devices["displays"])}))
        finally:
            app.terminate()
            try: app.wait(timeout=5)
            except subprocess.TimeoutExpired: app.kill(); app.wait()
