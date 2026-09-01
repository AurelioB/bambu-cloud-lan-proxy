from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_cloud_session_build_uses_vendored_source() -> None:
    dockerfile = (ROOT / "Dockerfile.gateway").read_text()

    assert "vendor/bambu-gateway/app" in dockerfile
    assert "vendor/bambu-gateway/tools/bambu_cloud_host" in dockerfile
    assert "vendor/bambu-gateway/web" in dockerfile
    assert "ghcr.io/leolobato/bambu-gateway" not in dockerfile
    assert "bambu-gateway/archive" not in dockerfile


def test_vendored_source_retains_license_and_provenance() -> None:
    vendor = ROOT / "vendor" / "bambu-gateway"

    assert (vendor / "LICENSE").is_file()
    assert "MIT License" in (vendor / "LICENSE").read_text()
    assert "79db09fff4929e9a7537453078031a51b3834c5f" in (
        vendor / "PROVENANCE.md"
    ).read_text()
