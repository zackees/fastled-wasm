from __future__ import annotations

from pathlib import Path

from fastled.toolchain import internal_wasm_build


def test_fast_compile_invalidates_cache_when_args_change(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    wrapper = tmp_path / "sketch.cpp"
    output = tmp_path / "sketch.o"
    wrapper.write_text("// wrapper\n", encoding="utf-8")

    cache_file = build_dir / "clang_compile_args.json"
    cache_key_file = build_dir / "clang_compile_args.key"
    cache_file.write_text(
        '["clang","-c","{input_cpp}","-o","{output_o}"]', encoding="utf-8"
    )
    cache_key_file.write_text("stale-key", encoding="utf-8")

    called = False

    def fail_run(*args, **kwargs):  # pragma: no cover - should never be called
        nonlocal called
        called = True
        raise AssertionError("subprocess.run should not be reached on key mismatch")

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fail_run)

    ok = internal_wasm_build._fast_compile(
        wrapper,
        output,
        build_dir,
        ["-c", str(wrapper), "-o", str(output), "-O2"],
    )

    assert ok is False
    assert called is False
    assert not cache_file.exists()
    assert not cache_key_file.exists()


def test_fast_compile_removes_stale_cache_on_oserror(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    wrapper = tmp_path / "sketch.cpp"
    output = tmp_path / "sketch.o"
    wrapper.write_text("// wrapper\n", encoding="utf-8")

    emcc_args = ["-c", str(wrapper), "-o", str(output), "-O2"]
    cache_file = build_dir / "clang_compile_args.json"
    cache_key_file = build_dir / "clang_compile_args.key"
    cache_file.write_text(
        '["missing-compiler","-c","{input_cpp}","-o","{output_o}"]', encoding="utf-8"
    )
    cache_key_file.write_text(
        internal_wasm_build._compile_cache_key(emcc_args), encoding="utf-8"
    )

    def raise_oserror(*args, **kwargs):
        raise OSError("stale compiler path")

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", raise_oserror)

    ok = internal_wasm_build._fast_compile(wrapper, output, build_dir, emcc_args)

    assert ok is False
    assert not cache_file.exists()
    assert not cache_key_file.exists()


def test_fast_link_invalidates_cache_when_library_key_changes(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    sketch_object = tmp_path / "sketch.o"
    cached_wasm = tmp_path / "fastled.wasm"
    library_archive = tmp_path / "libfastled.a"

    sketch_object.write_bytes(b"obj")
    cached_wasm.write_bytes(b"wasm")
    library_archive.write_bytes(b"archive")
    (build_dir / "fastled_glue.js").write_text("glue", encoding="utf-8")
    (build_dir / "libemscripten_js_symbols.so").write_text("stub", encoding="utf-8")
    cache_file = build_dir / "wasm_ld_args.json"
    cache_key_file = build_dir / "wasm_ld_args.key"
    cache_file.write_text(
        '["wasm-ld","{sketch_o}","-o","{output_wasm}"]', encoding="utf-8"
    )
    cache_key_file.write_text("stale-key", encoding="utf-8")
    (build_dir / "js_glue_fingerprint").write_text("current-js", encoding="utf-8")

    called = False

    def fail_run(*args, **kwargs):  # pragma: no cover - should never be called
        nonlocal called
        called = True
        raise AssertionError(
            "subprocess.run should not be reached on linker key mismatch"
        )

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fail_run)
    monkeypatch.setattr(
        internal_wasm_build, "_compute_js_glue_fingerprint", lambda: "current-js"
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_compute_link_environment_fingerprint",
        lambda mode: "current-env",
    )
    (build_dir / "link_environment_fingerprint").write_text(
        "current-env", encoding="utf-8"
    )

    ok = internal_wasm_build._fast_link(
        sketch_object, cached_wasm, build_dir, "quick", library_archive
    )

    assert ok is False
    assert called is False
    assert not cache_file.exists()
    assert not cache_key_file.exists()


def test_fast_link_invalidates_cache_when_key_missing_with_library_present(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    sketch_object = tmp_path / "sketch.o"
    cached_wasm = tmp_path / "fastled.wasm"
    library_archive = tmp_path / "libfastled.a"

    sketch_object.write_bytes(b"obj")
    cached_wasm.write_bytes(b"wasm")
    library_archive.write_bytes(b"archive")
    (build_dir / "fastled_glue.js").write_text("glue", encoding="utf-8")
    (build_dir / "libemscripten_js_symbols.so").write_text("stub", encoding="utf-8")
    cache_file = build_dir / "wasm_ld_args.json"
    cache_file.write_text(
        '["wasm-ld","{sketch_o}","-o","{output_wasm}"]', encoding="utf-8"
    )
    (build_dir / "js_glue_fingerprint").write_text("current-js", encoding="utf-8")

    called = False

    def fail_run(*args, **kwargs):  # pragma: no cover - should never be called
        nonlocal called
        called = True
        raise AssertionError(
            "subprocess.run should not be reached when key file is missing"
        )

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fail_run)
    monkeypatch.setattr(
        internal_wasm_build, "_compute_js_glue_fingerprint", lambda: "current-js"
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_compute_link_environment_fingerprint",
        lambda mode: "current-env",
    )
    (build_dir / "link_environment_fingerprint").write_text(
        "current-env", encoding="utf-8"
    )

    ok = internal_wasm_build._fast_link(
        sketch_object, cached_wasm, build_dir, "quick", library_archive
    )

    assert ok is False
    assert called is False
    assert not cache_file.exists()


def test_fast_link_cleans_up_cache_after_failed_link(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    sketch_object = tmp_path / "sketch.o"
    cached_wasm = tmp_path / "fastled.wasm"
    library_archive = tmp_path / "libfastled.a"

    sketch_object.write_bytes(b"obj")
    cached_wasm.write_bytes(b"wasm")
    library_archive.write_bytes(b"archive")
    (build_dir / "fastled_glue.js").write_text("glue", encoding="utf-8")
    (build_dir / "libemscripten_js_symbols.so").write_text("stub", encoding="utf-8")
    cache_file = build_dir / "wasm_ld_args.json"
    cache_key_file = build_dir / "wasm_ld_args.key"
    cache_file.write_text(
        '["wasm-ld","{sketch_o}","--strip-debug","-o","{output_wasm}"]',
        encoding="utf-8",
    )
    cache_key_file.write_text(
        internal_wasm_build._link_cache_key(library_archive), encoding="utf-8"
    )
    (build_dir / "js_glue_fingerprint").write_text("current-js", encoding="utf-8")

    commands: list[list[str]] = []

    class Result:
        returncode = 1

    def fake_run(cmd, cwd=None):
        commands.append(cmd)
        return Result()

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fake_run)
    monkeypatch.setattr(
        internal_wasm_build, "_compute_js_glue_fingerprint", lambda: "current-js"
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_compute_link_environment_fingerprint",
        lambda mode: "current-env",
    )
    (build_dir / "link_environment_fingerprint").write_text(
        "current-env", encoding="utf-8"
    )

    ok = internal_wasm_build._fast_link(
        sketch_object, cached_wasm, build_dir, "quick", library_archive
    )

    assert ok is False
    assert commands
    assert "--strip-all" in commands[0]
    assert not cache_file.exists()
    assert not cache_key_file.exists()


def test_fast_link_invalidates_cache_when_js_glue_fingerprint_changes(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    sketch_object = tmp_path / "sketch.o"
    cached_wasm = tmp_path / "fastled.wasm"
    library_archive = tmp_path / "libfastled.a"

    sketch_object.write_bytes(b"obj")
    cached_wasm.write_bytes(b"wasm")
    library_archive.write_bytes(b"archive")
    (build_dir / "fastled_glue.js").write_text("glue", encoding="utf-8")
    (build_dir / "libemscripten_js_symbols.so").write_text("stub", encoding="utf-8")
    (build_dir / "wasm_ld_args.json").write_text(
        '["wasm-ld","{sketch_o}","-o","{output_wasm}"]', encoding="utf-8"
    )
    (build_dir / "wasm_ld_args.key").write_text(
        internal_wasm_build._link_cache_key(library_archive), encoding="utf-8"
    )
    js_fingerprint_file = build_dir / "js_glue_fingerprint"
    js_fingerprint_file.write_text("stale-js", encoding="utf-8")

    called = False

    def fail_run(*args, **kwargs):
        nonlocal called
        called = True
        raise AssertionError("subprocess.run should not be reached on JS mismatch")

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fail_run)
    monkeypatch.setattr(
        internal_wasm_build, "_compute_js_glue_fingerprint", lambda: "fresh-js"
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_compute_link_environment_fingerprint",
        lambda mode: "current-env",
    )
    (build_dir / "link_environment_fingerprint").write_text(
        "current-env", encoding="utf-8"
    )

    ok = internal_wasm_build._fast_link(
        sketch_object, cached_wasm, build_dir, "quick", library_archive
    )

    assert ok is False
    assert called is False
    assert not (build_dir / "wasm_ld_args.json").exists()
    assert not (build_dir / "wasm_ld_args.key").exists()
    assert not js_fingerprint_file.exists()


def test_fast_link_uses_cache_when_js_glue_fingerprint_matches(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    sketch_object = tmp_path / "sketch.o"
    cached_wasm = tmp_path / "fastled.wasm"
    library_archive = tmp_path / "libfastled.a"

    sketch_object.write_bytes(b"obj")
    cached_wasm.write_bytes(b"wasm")
    library_archive.write_bytes(b"archive")
    (build_dir / "fastled_glue.js").write_text("glue", encoding="utf-8")
    (build_dir / "libemscripten_js_symbols.so").write_text("stub", encoding="utf-8")
    (build_dir / "wasm_ld_args.json").write_text(
        '["wasm-ld","{sketch_o}","-o","{output_wasm}"]', encoding="utf-8"
    )
    (build_dir / "wasm_ld_args.key").write_text(
        internal_wasm_build._link_cache_key(library_archive), encoding="utf-8"
    )
    (build_dir / "js_glue_fingerprint").write_text("current-js", encoding="utf-8")

    commands: list[list[str]] = []

    class Result:
        returncode = 0

    def fake_run(cmd, cwd=None):
        commands.append(cmd)
        return Result()

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fake_run)
    monkeypatch.setattr(
        internal_wasm_build, "_compute_js_glue_fingerprint", lambda: "current-js"
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_compute_link_environment_fingerprint",
        lambda mode: "current-env",
    )
    (build_dir / "link_environment_fingerprint").write_text(
        "current-env", encoding="utf-8"
    )

    ok = internal_wasm_build._fast_link(
        sketch_object, cached_wasm, build_dir, "quick", library_archive
    )

    assert ok is True
    assert commands
    assert (cached_wasm.parent / "fastled.js").read_text(encoding="utf-8") == "glue"


def test_fast_link_invalidates_cache_when_link_environment_changes(
    tmp_path: Path, monkeypatch
) -> None:
    build_dir = tmp_path / "build"
    build_dir.mkdir()
    sketch_object = tmp_path / "sketch.o"
    cached_wasm = tmp_path / "fastled.wasm"
    library_archive = tmp_path / "libfastled.a"

    sketch_object.write_bytes(b"obj")
    cached_wasm.write_bytes(b"wasm")
    library_archive.write_bytes(b"archive")
    (build_dir / "fastled_glue.js").write_text("glue", encoding="utf-8")
    (build_dir / "libemscripten_js_symbols.so").write_text("stub", encoding="utf-8")
    (build_dir / "wasm_ld_args.json").write_text(
        '["wasm-ld","{sketch_o}","-o","{output_wasm}"]', encoding="utf-8"
    )
    (build_dir / "wasm_ld_args.key").write_text(
        internal_wasm_build._link_cache_key(library_archive), encoding="utf-8"
    )
    (build_dir / "js_glue_fingerprint").write_text("current-js", encoding="utf-8")
    env_fingerprint_file = build_dir / "link_environment_fingerprint"
    env_fingerprint_file.write_text("stale-env", encoding="utf-8")

    called = False

    def fail_run(*args, **kwargs):
        nonlocal called
        called = True
        raise AssertionError("subprocess.run should not be reached on env mismatch")

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fail_run)
    monkeypatch.setattr(
        internal_wasm_build, "_compute_js_glue_fingerprint", lambda: "current-js"
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_compute_link_environment_fingerprint",
        lambda mode: "fresh-env",
    )

    ok = internal_wasm_build._fast_link(
        sketch_object, cached_wasm, build_dir, "quick", library_archive
    )

    assert ok is False
    assert called is False
    assert not (build_dir / "wasm_ld_args.json").exists()
    assert not (build_dir / "wasm_ld_args.key").exists()
    assert not (build_dir / "js_glue_fingerprint").exists()
    assert not env_fingerprint_file.exists()


def test_link_environment_fingerprint_changes_when_emcc_version_changes(
    monkeypatch,
) -> None:
    monkeypatch.setattr(internal_wasm_build, "get_link_flags", lambda mode: ["-O1"])
    monkeypatch.setattr(internal_wasm_build, "get_emcc", lambda: "emcc")
    monkeypatch.setattr(
        internal_wasm_build.Path,
        "stat",
        lambda self: type("Stat", (), {"st_mtime": 1.0, "st_size": 2})(),
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_build_env",
        lambda: {},
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_cached_emcc_version_key",
        None,
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_cached_emcc_version_value",
        None,
    )
    monkeypatch.setattr(
        internal_wasm_build,
        "_cached_emcc_version_time",
        0.0,
    )

    versions = iter(["emcc 1.0.0", "emcc 2.0.0"])

    class Result:
        def __init__(self, text: str) -> None:
            self.stdout = text
            self.stderr = ""

    def fake_run(cmd, capture_output=None, text=None, env=None):
        return Result(next(versions))

    monkeypatch.setattr(internal_wasm_build.subprocess, "run", fake_run)

    first = internal_wasm_build._compute_link_environment_fingerprint("quick")
    monkeypatch.setattr(internal_wasm_build, "_cached_emcc_version_time", 0.0)
    second = internal_wasm_build._compute_link_environment_fingerprint("quick")

    assert first != second
