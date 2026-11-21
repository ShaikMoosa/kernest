"""Tests for the kernest scripting package. Run with the python dir on the path:

    PYTHONPATH=python python -m unittest discover -s python/tests
"""

import json
import os
import subprocess
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import kernest  # noqa: E402

PKG_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def fixture():
    return {
        "info": {"familyName": "Test"},
        "masters": [{"id": "m01", "name": "Regular"}],
        "glyphs": [
            {
                "name": "A",
                "unicodes": ["0041"],
                "layers": [
                    {
                        "masterId": "m01",
                        "width": 500,
                        "shapes": [
                            {"type": "path", "closed": True,
                             "nodes": ["100 0 line", "500 0 line", "500 700 line", "100 700 line"]},
                            {"type": "component", "base": "B"},
                        ],
                        "anchors": [{"name": "top", "x": 300, "y": 700}],
                    }
                ],
            }
        ],
    }


class WrapperTests(unittest.TestCase):
    def setUp(self):
        self.data = fixture()
        self.font = kernest.Font(self.data, {"glyphs": ["A"], "masterId": "m01", "layers": ["m01"]})

    def test_width_round_trips_in_place(self):
        layer = self.font.glyphs["A"].layers["m01"]
        self.assertEqual(layer.width, 500)
        layer.width = 600
        self.assertEqual(self.data["glyphs"][0]["layers"][0]["width"], 600)

    def test_master_layer_found_by_master_id(self):
        self.assertIn("m01", self.font.glyphs["A"].layers)
        self.assertEqual(self.font.glyphs["A"].layers["m01"].master_id, "m01")
        self.assertEqual(list(self.font.glyphs["A"].layers.keys()), ["m01"])

    def test_paths_and_components_split(self):
        layer = self.font.glyphs["A"].layers["m01"]
        self.assertEqual(len(layer.paths), 1)
        self.assertEqual(len(layer.components), 1)
        self.assertTrue(layer.paths[0].closed)
        self.assertEqual(layer.components[0].base, "B")

    def test_nodes_parse_and_write_back(self):
        nodes = self.font.glyphs["A"].layers["m01"].paths[0].nodes
        self.assertEqual(len(nodes), 4)
        self.assertEqual((nodes[1].x, nodes[1].y, nodes[1].kind), (500, 0, "line"))
        nodes[1].x = 550
        self.assertEqual(self.data["glyphs"][0]["layers"][0]["shapes"][0]["nodes"][1], "550 0 line")
        nodes[0].smooth = True
        self.assertEqual(self.data["glyphs"][0]["layers"][0]["shapes"][0]["nodes"][0], "100 0 line smooth")

    def test_super_smooth_nodes_keep_their_flag(self):
        raw = self.data["glyphs"][0]["layers"][0]["shapes"][0]["nodes"]
        raw[2] = "300 700 curve smooth g2"
        nodes = self.font.glyphs["A"].layers["m01"].paths[0].nodes
        self.assertTrue(nodes[2].super_smooth and nodes[2].smooth)
        nodes[2].x = 310
        self.assertEqual(raw[2], "310 700 curve smooth g2")
        nodes[2].smooth = False
        self.assertEqual(raw[2], "310 700 curve")
        self.assertFalse(nodes[2].super_smooth)

    def test_node_append_formats_whole_numbers(self):
        nodes = self.font.glyphs["A"].layers["m01"].paths[0].nodes
        nodes.append(300, 350, "curve")
        self.assertEqual(self.data["glyphs"][0]["layers"][0]["shapes"][0]["nodes"][-1], "300 350 curve")

    def test_anchors_by_name_and_append(self):
        anchors = self.font.glyphs["A"].layers["m01"].anchors
        self.assertEqual(anchors["top"].x, 300)
        anchors["top"].y = 750
        self.assertEqual(self.data["glyphs"][0]["layers"][0]["anchors"][0]["y"], 750)
        anchors.append("bottom", 300, 0)
        self.assertEqual(len(anchors), 2)

    def test_selection(self):
        self.assertEqual(self.font.selected_glyphs, ["A"])
        self.assertEqual(self.font.selected_layers, ["m01"])
        self.assertEqual(self.font.current_glyph, None)
        self.assertEqual(self.font.master_id, "m01")

    def test_unknown_fields_survive(self):
        self.data["glyphs"][0]["layers"][0]["custom"] = {"keep": 1}
        self.font.glyphs["A"].layers["m01"].width = 1
        self.assertEqual(self.data["glyphs"][0]["layers"][0]["custom"], {"keep": 1})

    def test_reporter_helpers(self):
        kernest._draw.clear()
        kernest.report(kernest.line(0, 0, 100, 700, color="#f00"),
                       kernest.text(10, 20, "hi", size=12))
        drawn = kernest._take_draw()
        self.assertEqual(len(drawn), 2)
        self.assertEqual(drawn[0]["kind"], "line")
        self.assertEqual(drawn[0]["color"], "#f00")
        self.assertEqual(drawn[1]["text"], "hi")
        self.assertEqual(kernest._take_draw(), [])


class BootstrapTests(unittest.TestCase):
    def run_code(self, code, kind="macro", font=None, selection=None):
        request = {
            "protocol": 1, "kind": kind,
            "font": font if font is not None else fixture(),
            "selection": selection or {"glyphs": ["A"], "masterId": "m01"},
            "code": code,
        }
        env = dict(os.environ, PYTHONPATH=PKG_DIR, PYTHONIOENCODING="utf-8")
        proc = subprocess.run(
            [sys.executable, "-u", "-m", "kernest"],
            input=json.dumps(request), capture_output=True, text=True, env=env, timeout=30,
        )
        messages = [json.loads(l) for l in proc.stdout.splitlines() if l.strip()]
        return messages, proc

    def test_macro_changes_font(self):
        msgs, proc = self.run_code("font.glyphs['A'].layers['m01'].width = 999")
        result = [m for m in msgs if m["type"] == "result"][-1]
        self.assertTrue(result["ok"], proc.stderr)
        self.assertEqual(result["font"]["glyphs"][0]["layers"][0]["width"], 999)

    def test_print_streams(self):
        msgs, _ = self.run_code("print('hello')")
        out = [m for m in msgs if m["type"] == "output"]
        self.assertTrue(any(m["text"] == "hello" for m in out))

    def test_error_is_reported(self):
        msgs, _ = self.run_code("raise ValueError('boom')")
        result = [m for m in msgs if m["type"] == "result"][-1]
        self.assertFalse(result["ok"])
        self.assertIn("boom", result["error"])

    def test_reporter_returns_draw(self):
        msgs, _ = self.run_code("report(line(0, 0, 100, 700))", kind="reporter")
        result = [m for m in msgs if m["type"] == "result"][-1]
        self.assertTrue(result["ok"])
        self.assertEqual(len(result["draw"]), 1)
        self.assertEqual(result["draw"][0]["kind"], "line")


if __name__ == "__main__":
    unittest.main()