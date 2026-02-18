#!/usr/bin/env python3
"""
Voice Input -- Launcher & Test Dashboard

A Python script to:
1. Check prerequisites (Rust, Bun)
2. Install dependencies
3. Launch the Tauri dev server with UI
4. Provide a dashboard for monitoring and testing

Usage:
    python run_voice_input.py              # Launch full Tauri app
    python run_voice_input.py --frontend   # Launch frontend only (Vite dev)
    python run_voice_input.py --test       # Run all tests
    python run_voice_input.py --test-rust  # Run Rust tests only
    python run_voice_input.py --dashboard  # Launch monitoring dashboard
"""

import subprocess
import sys
import os
import shutil
import threading
import time
import argparse
import webbrowser
import io
from pathlib import Path

# Fix Windows console encoding for Unicode
if sys.platform == "win32":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

# ── Constants ────────────────────────────────────────────────────────
PROJECT_ROOT = Path(__file__).parent.resolve()
SRC_TAURI = PROJECT_ROOT / "src-tauri"
VITE_URL = "http://localhost:1420"
VAD_MODEL_URL = "https://blob.handy.computer/silero_vad_v4.onnx"
VAD_MODEL_PATH = SRC_TAURI / "resources" / "models" / "silero_vad_v4.onnx"


# ── Terminal Colors ──────────────────────────────────────────────────
class C:
    GREEN = "\033[92m"
    YELLOW = "\033[93m"
    RED = "\033[91m"
    CYAN = "\033[96m"
    BOLD = "\033[1m"
    END = "\033[0m"


def ok(msg):
    print(f"  {C.GREEN}[OK]{C.END} {msg}")


def warn(msg):
    print(f"  {C.YELLOW}[!!]{C.END} {msg}")


def err(msg):
    print(f"  {C.RED}[ERR]{C.END} {msg}")


def info(msg):
    print(f"  {C.CYAN}[>>]{C.END} {msg}")


def header(msg):
    print(f"\n{C.BOLD}{C.CYAN}{'-' * 60}{C.END}")
    print(f"  {C.BOLD}{msg}{C.END}")
    print(f"{C.BOLD}{C.CYAN}{'-' * 60}{C.END}")


# ── Prerequisite Checks ─────────────────────────────────────────────
def check_command(name, version_flag="--version"):
    """Check if a command is available and return its version."""
    path = shutil.which(name)
    if not path:
        return None, None
    try:
        result = subprocess.run(
            [path, version_flag],
            capture_output=True,
            text=True,
            timeout=10,
        )
        version = (result.stdout.strip() or result.stderr.strip()).split("\n")[0]
        return path, version
    except Exception:
        return path, "unknown"


def check_prerequisites():
    """Check all required tools are installed."""
    header("Checking Prerequisites")

    all_ok = True

    # Rust
    path, ver = check_command("rustc")
    if path:
        ok(f"Rust: {ver}")
    else:
        err("Rust not found. Install from https://rustup.rs/")
        all_ok = False

    # Cargo
    path, ver = check_command("cargo")
    if path:
        ok(f"Cargo: {ver}")
    else:
        err("Cargo not found")
        all_ok = False

    # Bun
    path, ver = check_command("bun")
    if path:
        ok(f"Bun: {ver}")
    else:
        err("Bun not found. Install from https://bun.sh/")
        all_ok = False

    # Node (optional, for some tools)
    path, ver = check_command("node")
    if path:
        ok(f"Node.js: {ver}")
    else:
        warn("Node.js not found (optional)")

    # VAD Model
    if VAD_MODEL_PATH.exists():
        size_mb = VAD_MODEL_PATH.stat().st_size / (1024 * 1024)
        ok(f"VAD model: {VAD_MODEL_PATH.name} ({size_mb:.1f} MB)")
    else:
        warn(f"VAD model not found at {VAD_MODEL_PATH}")
        warn("Will attempt to download...")

    return all_ok


# ── Setup ────────────────────────────────────────────────────────────
def install_dependencies():
    """Install frontend dependencies with Bun."""
    header("Installing Dependencies")

    info("Running bun install...")
    result = subprocess.run(
        ["bun", "install"],
        cwd=str(PROJECT_ROOT),
        capture_output=False,
    )
    if result.returncode == 0:
        ok("Frontend dependencies installed")
    else:
        err("Failed to install dependencies")
        return False

    return True


def download_vad_model():
    """Download the VAD model if missing."""
    if VAD_MODEL_PATH.exists():
        return True

    header("Downloading VAD Model")
    info(f"Downloading from {VAD_MODEL_URL}...")

    VAD_MODEL_PATH.parent.mkdir(parents=True, exist_ok=True)

    try:
        # Try with curl first (more reliable for large files)
        curl_path = shutil.which("curl")
        if curl_path:
            result = subprocess.run(
                ["curl", "-L", "-o", str(VAD_MODEL_PATH), VAD_MODEL_URL],
                capture_output=False,
            )
            if result.returncode == 0:
                ok(f"VAD model downloaded to {VAD_MODEL_PATH}")
                return True

        # Fallback to Python's urllib
        import urllib.request
        urllib.request.urlretrieve(VAD_MODEL_URL, str(VAD_MODEL_PATH))
        ok(f"VAD model downloaded to {VAD_MODEL_PATH}")
        return True
    except Exception as e:
        err(f"Failed to download VAD model: {e}")
        return False


# ── Test Runners ─────────────────────────────────────────────────────
def run_rust_tests():
    """Run Rust unit tests."""
    header("Running Rust Tests")

    result = subprocess.run(
        ["cargo", "test", "--lib"],
        cwd=str(SRC_TAURI),
        capture_output=False,
    )

    if result.returncode == 0:
        ok("All Rust tests passed!")
    else:
        err("Some Rust tests failed")
    return result.returncode == 0


def run_frontend_lint():
    """Run frontend linting."""
    header("Running Frontend Lint")

    result = subprocess.run(
        ["bun", "run", "lint"],
        cwd=str(PROJECT_ROOT),
        capture_output=False,
    )

    if result.returncode == 0:
        ok("Frontend lint passed!")
    else:
        warn("Frontend lint had issues")
    return result.returncode == 0


def run_format_check():
    """Check code formatting."""
    header("Checking Code Format")

    result = subprocess.run(
        ["bun", "run", "format:check"],
        cwd=str(PROJECT_ROOT),
        capture_output=False,
    )

    if result.returncode == 0:
        ok("Code format is correct!")
    else:
        warn("Code formatting issues found. Run: bun run format")
    return result.returncode == 0


def run_type_check():
    """Run TypeScript type checking."""
    header("Running TypeScript Type Check")

    result = subprocess.run(
        ["bun", "x", "tsc", "--noEmit"],
        cwd=str(PROJECT_ROOT),
        capture_output=False,
    )

    if result.returncode == 0:
        ok("TypeScript types OK!")
    else:
        warn("TypeScript type errors found")
    return result.returncode == 0


def run_all_tests():
    """Run all tests."""
    header("Running All Tests")

    results = {}
    results["Rust Tests"] = run_rust_tests()
    results["Frontend Lint"] = run_frontend_lint()
    results["Type Check"] = run_type_check()
    results["Format Check"] = run_format_check()

    header("Test Summary")
    all_passed = True
    for name, passed in results.items():
        if passed:
            ok(name)
        else:
            err(name)
            all_passed = False

    if all_passed:
        print(f"\n  {C.GREEN}{C.BOLD}All checks passed!{C.END}")
    else:
        print(f"\n  {C.YELLOW}{C.BOLD}Some checks had issues.{C.END}")

    return all_passed


# ── Launchers ────────────────────────────────────────────────────────
def launch_frontend_only():
    """Launch the Vite dev server (frontend only, no Tauri)."""
    header("Launching Frontend Dev Server")
    info(f"Starting Vite at {VITE_URL}")
    info("Press Ctrl+C to stop")

    # Open browser after a short delay
    def open_browser():
        time.sleep(3)
        webbrowser.open(VITE_URL)

    threading.Thread(target=open_browser, daemon=True).start()

    try:
        subprocess.run(
            ["bun", "run", "dev"],
            cwd=str(PROJECT_ROOT),
        )
    except KeyboardInterrupt:
        info("Frontend server stopped")


def launch_tauri_dev():
    """Launch the full Tauri development app."""
    header("Launching Voice Input (Tauri Dev)")
    info("Building Rust backend + starting frontend...")
    info("This may take a few minutes on first run")
    info("Press Ctrl+C to stop")

    try:
        subprocess.run(
            ["bun", "run", "tauri", "dev"],
            cwd=str(PROJECT_ROOT),
        )
    except KeyboardInterrupt:
        info("Tauri dev stopped")


# ── Dashboard (tkinter) ─────────────────────────────────────────────
def launch_dashboard():
    """Launch a simple tkinter dashboard for monitoring and testing."""
    try:
        import tkinter as tk
        from tkinter import ttk, scrolledtext
    except ImportError:
        err("tkinter not available. Install python3-tk or use --test instead.")
        return

    root = tk.Tk()
    root.title("Voice Input — Test Dashboard")
    root.geometry("800x600")
    root.configure(bg="#1e1e2e")

    style = ttk.Style()
    style.theme_use("clam")
    style.configure("TButton", padding=8, font=("Segoe UI", 10))
    style.configure("TLabel", background="#1e1e2e", foreground="#cdd6f4", font=("Segoe UI", 10))
    style.configure("Header.TLabel", font=("Segoe UI", 14, "bold"), foreground="#89b4fa")
    style.configure("Status.TLabel", font=("Segoe UI", 10))
    style.configure("Green.TLabel", foreground="#a6e3a1")
    style.configure("Red.TLabel", foreground="#f38ba8")
    style.configure("Yellow.TLabel", foreground="#f9e2af")

    # Header
    header_frame = ttk.Frame(root)
    header_frame.pack(fill=tk.X, padx=20, pady=(15, 5))
    ttk.Label(header_frame, text="Voice Input — Test Dashboard", style="Header.TLabel").pack(
        anchor=tk.W
    )
    ttk.Label(
        header_frame,
        text=f"Project: {PROJECT_ROOT}",
        style="TLabel",
    ).pack(anchor=tk.W)

    # Output area
    output_frame = ttk.LabelFrame(root, text="Output", padding=5)
    output_frame.pack(fill=tk.BOTH, expand=True, padx=20, pady=10)

    output = scrolledtext.ScrolledText(
        output_frame,
        bg="#181825",
        fg="#cdd6f4",
        insertbackground="#cdd6f4",
        font=("Consolas", 9),
        wrap=tk.WORD,
    )
    output.pack(fill=tk.BOTH, expand=True)

    # Status bar
    status_var = tk.StringVar(value="Ready")
    status_label = ttk.Label(root, textvariable=status_var, style="Status.TLabel")
    status_label.pack(fill=tk.X, padx=20, pady=(0, 5))

    def write_output(text, tag=None):
        output.insert(tk.END, text + "\n", tag)
        output.see(tk.END)
        root.update_idletasks()

    def clear_output():
        output.delete(1.0, tk.END)

    def run_in_thread(func, label):
        def wrapper():
            status_var.set(f"Running: {label}...")
            clear_output()
            write_output(f"{'=' * 50}")
            write_output(f"  {label}")
            write_output(f"{'=' * 50}\n")

            try:
                result = subprocess.run(
                    func,
                    cwd=str(PROJECT_ROOT if "cargo" not in func else SRC_TAURI),
                    capture_output=True,
                    text=True,
                    timeout=300,
                )
                if result.stdout:
                    write_output(result.stdout)
                if result.stderr:
                    write_output(result.stderr)

                if result.returncode == 0:
                    write_output(f"\n✓ {label} — PASSED", "green")
                    status_var.set(f"✓ {label} passed")
                else:
                    write_output(f"\n✗ {label} — FAILED (exit code {result.returncode})", "red")
                    status_var.set(f"✗ {label} failed")
            except subprocess.TimeoutExpired:
                write_output(f"\n! {label} — TIMEOUT", "yellow")
                status_var.set(f"! {label} timed out")
            except Exception as e:
                write_output(f"\n✗ Error: {e}", "red")
                status_var.set(f"✗ Error running {label}")

        threading.Thread(target=wrapper, daemon=True).start()

    # Configure text tags for colors
    output.tag_configure("green", foreground="#a6e3a1")
    output.tag_configure("red", foreground="#f38ba8")
    output.tag_configure("yellow", foreground="#f9e2af")

    # Button panel
    btn_frame = ttk.Frame(root)
    btn_frame.pack(fill=tk.X, padx=20, pady=(0, 15))

    buttons = [
        ("Run Rust Tests", ["cargo", "test", "--lib"]),
        ("Lint Frontend", ["bun", "run", "lint"]),
        ("Type Check", ["bun", "x", "tsc", "--noEmit"]),
        ("Format Check", ["bun", "run", "format:check"]),
    ]

    for i, (label, cmd) in enumerate(buttons):
        btn = ttk.Button(btn_frame, text=label, command=lambda c=cmd, l=label: run_in_thread(c, l))
        btn.grid(row=0, column=i, padx=5, sticky=tk.EW)
        btn_frame.columnconfigure(i, weight=1)

    # Second row of buttons
    btn_frame2 = ttk.Frame(root)
    btn_frame2.pack(fill=tk.X, padx=20, pady=(0, 15))

    def launch_frontend_btn():
        status_var.set("Starting frontend dev server...")
        clear_output()
        write_output("Starting Vite dev server at http://localhost:1420...\n")
        write_output("Opening browser in 3 seconds...\n")

        def runner():
            time.sleep(2)
            webbrowser.open(VITE_URL)

        threading.Thread(target=runner, daemon=True).start()
        run_in_thread(["bun", "run", "dev"], "Vite Dev Server")

    def launch_tauri_btn():
        status_var.set("Building & launching Tauri app...")
        clear_output()
        write_output("Starting Tauri dev mode (this may take a few minutes)...\n")
        run_in_thread(["bun", "run", "tauri", "dev"], "Tauri Dev")

    def run_all_btn():
        status_var.set("Running all checks...")
        clear_output()

        def run_all_thread():
            checks = [
                ("Rust Tests", ["cargo", "test", "--lib"]),
                ("Frontend Lint", ["bun", "run", "lint"]),
                ("Type Check", ["bun", "x", "tsc", "--noEmit"]),
            ]

            results = {}
            for label, cmd in checks:
                write_output(f"\n{'─' * 40}")
                write_output(f"  Running: {label}")
                write_output(f"{'─' * 40}\n")
                status_var.set(f"Running: {label}...")

                try:
                    result = subprocess.run(
                        cmd,
                        cwd=str(SRC_TAURI if "cargo" in cmd else PROJECT_ROOT),
                        capture_output=True,
                        text=True,
                        timeout=300,
                    )
                    if result.stdout:
                        write_output(result.stdout)
                    if result.stderr:
                        write_output(result.stderr)
                    results[label] = result.returncode == 0
                except Exception as e:
                    write_output(f"Error: {e}", "red")
                    results[label] = False

            write_output(f"\n{'═' * 50}")
            write_output("  Summary")
            write_output(f"{'═' * 50}\n")

            for label, passed in results.items():
                if passed:
                    write_output(f"  ✓ {label}", "green")
                else:
                    write_output(f"  ✗ {label}", "red")

            all_passed = all(results.values())
            if all_passed:
                write_output(f"\n  All checks passed!", "green")
                status_var.set("✓ All checks passed!")
            else:
                write_output(f"\n  Some checks failed.", "red")
                status_var.set("✗ Some checks failed")

        threading.Thread(target=run_all_thread, daemon=True).start()

    buttons2 = [
        ("Launch Frontend", launch_frontend_btn),
        ("Launch Tauri App", launch_tauri_btn),
        ("Run All Checks", run_all_btn),
        ("Clear Output", clear_output),
    ]

    for i, (label, cmd_fn) in enumerate(buttons2):
        btn = ttk.Button(btn_frame2, text=label, command=cmd_fn)
        btn.grid(row=0, column=i, padx=5, sticky=tk.EW)
        btn_frame2.columnconfigure(i, weight=1)

    root.mainloop()


# ── Main ─────────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(
        description="Voice Input -- Launcher & Test Dashboard",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python run_voice_input.py               Launch full Tauri app
  python run_voice_input.py --frontend    Launch frontend only
  python run_voice_input.py --test        Run all tests
  python run_voice_input.py --test-rust   Run Rust tests only
  python run_voice_input.py --dashboard   Launch test dashboard GUI
        """,
    )
    parser.add_argument(
        "--frontend", action="store_true", help="Launch frontend only (Vite dev server)"
    )
    parser.add_argument(
        "--test", action="store_true", help="Run all tests"
    )
    parser.add_argument(
        "--test-rust", action="store_true", help="Run Rust tests only"
    )
    parser.add_argument(
        "--dashboard", action="store_true", help="Launch test dashboard GUI"
    )
    parser.add_argument(
        "--skip-checks", action="store_true", help="Skip prerequisite checks"
    )

    args = parser.parse_args()

    print(f"\n{C.BOLD}{C.CYAN}  Voice Input v0.8.0 -- Launcher{C.END}")

    # Check prerequisites
    if not args.skip_checks:
        if not check_prerequisites():
            err("Missing prerequisites. Install them and try again.")
            sys.exit(1)

    # Handle modes
    if args.test_rust:
        success = run_rust_tests()
        sys.exit(0 if success else 1)

    if args.test:
        success = run_all_tests()
        sys.exit(0 if success else 1)

    if args.dashboard:
        install_dependencies()
        download_vad_model()
        launch_dashboard()
        return

    if args.frontend:
        install_dependencies()
        launch_frontend_only()
        return

    # Default: full Tauri dev launch
    install_dependencies()
    download_vad_model()
    launch_tauri_dev()


if __name__ == "__main__":
    main()
