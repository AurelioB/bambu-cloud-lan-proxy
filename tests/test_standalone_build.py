import ast
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_no_gateway_application_dependency():
    assert not any((ROOT / "vendor" / "bambu-gateway").rglob("*.*"))
    dockerfile = (ROOT / "Dockerfile.proxy").read_text()
    assert "vendor/" not in dockerfile
    assert "leolobato" not in dockerfile.lower()
    assert "node:" not in dockerfile
    assert "bambu_cloud_lan_proxy.service" in dockerfile
    for path in (ROOT / "src").rglob("*.py"):
        for node in ast.walk(ast.parse(path.read_text())):
            if isinstance(node, ast.ImportFrom):
                assert (node.module or "").split(".")[0] != "app", path
            if isinstance(node, ast.Import):
                assert all(alias.name.split(".")[0] != "app" for alias in node.names), path


def test_first_party_native_host_and_reference_attribution():
    assert (ROOT / "native" / "main.cpp").is_file()
    assert "Leonardo Lobato" in (ROOT / "THIRD_PARTY_NOTICES").read_text()
