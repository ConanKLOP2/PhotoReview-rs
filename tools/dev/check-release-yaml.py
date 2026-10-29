"""Optional PyYAML syntax/shape check; `on` must retain its YAML spelling."""
import sys
import yaml

with open(sys.argv[1], encoding="utf-8") as stream:
    workflow = yaml.load(stream, Loader=yaml.BaseLoader)
assert set(workflow["on"]) == {"workflow_dispatch"}, "release must be dispatch-only"
assert workflow["jobs"]["release"]["timeout-minutes"], "release needs a timeout"
print("PASS: PyYAML release syntax and dispatch-only trigger")
