from pathlib import Path

from fastled.server_flask import create_app


def test_dwarfsource_route_returns_file_contents(tmp_path: Path) -> None:
    sketch_dir = tmp_path / "sketch"
    output_dir = sketch_dir / "fastled_js"
    output_dir.mkdir(parents=True)
    (output_dir / "index.html").write_text("<html></html>")
    source_file = sketch_dir / "demo.cpp"
    source_file.write_text("int main() {}")

    app = create_app(output_dir, sketch_dir=sketch_dir)
    client = app.test_client()
    response = client.post("/dwarfsource", json={"path": "sketchsource/demo.cpp"})

    assert response.status_code == 200
    assert response.get_data(as_text=True) == "int main() {}"


def test_dwarfsource_route_rejects_invalid_requests(tmp_path: Path) -> None:
    output_dir = tmp_path / "fastled_js"
    output_dir.mkdir()
    (output_dir / "index.html").write_text("<html></html>")

    app = create_app(output_dir, sketch_dir=tmp_path)
    client = app.test_client()

    missing_path = client.post("/dwarfsource", json={})
    assert missing_path.status_code == 400

    traversal = client.post("/dwarfsource", json={"path": "dwarfsource/../secret.txt"})
    assert traversal.status_code == 400
