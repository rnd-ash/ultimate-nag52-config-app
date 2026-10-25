"""Exercise verifier exit codes without a desktop or a compiled application."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]


class GuiEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tools = self.root / "bin"
        self.tools.mkdir()
        self.summary = self.root / "summary"
        self.env = dict(os.environ, PATH=f"{self.tools}:{os.environ['PATH']}",
                        GITHUB_STEP_SUMMARY=str(self.summary), MACOSX_DEPLOYMENT_TARGET="11.0")

    def tool(self, name, body):
        path = self.tools / name
        path.write_text("#!/bin/bash\nset -eu\n" + body + "\n")
        path.chmod(0o755)
        return path

    def run_verifier(self, platform, artifact, mode):
        return subprocess.run(["bash", str(SCRIPTS / f"verify-{platform}.sh"), str(artifact)],
                              env=dict(self.env, TEST_MODE=mode), capture_output=True, text=True)

    def linux(self, mode):
        self.tool("desktop-file-validate", "true")
        self.tool("ldd", "echo 'libc.so.6 => /lib/libc.so.6'")
        self.tool("readelf", "echo GLIBC_2.35")
        for tool in ("Xvfb", "xauth"):
            self.tool(tool, "true")
        self.tool("xvfb-run", '[ "$TEST_MODE" != server-fails ] || exit 1\nshift\nexec "$@"')
        self.tool("glxinfo", '''[ "$TEST_MODE" != gl-fails ] || exit 1
if [ "$TEST_MODE" = old-gl ]; then
    echo 'OpenGL core profile version string: 3.1 Mesa'
else
    echo 'OpenGL core profile version string: 4.5 Mesa'
fi''')
        self.tool("xdotool", '[ "$1" = getdisplaygeometry ]')
        self.tool("timeout", 'shift\nexec "$@"')
        image = self.tool("fixture.AppImage", '''if [ "$1" = --appimage-extract ]; then
    mkdir -p squashfs-root/usr/{bin,share/applications,share/pixmaps}
    touch squashfs-root/{AppRun,config_app.desktop} squashfs-root/usr/bin/config_app
    touch squashfs-root/usr/share/{applications/config_app.desktop,pixmaps/config_app.png}
    chmod +x squashfs-root/AppRun squashfs-root/usr/bin/config_app
    [ "$TEST_MODE" != bad-package ] || rm squashfs-root/config_app.desktop
else
    exit 1
fi''')
        return self.run_verifier("linux-appimage", image, mode)

    def macos(self, mode):
        self.tool("hdiutil", '''if [ "$1" = verify ]; then
    [ "$TEST_MODE" != bad-package ]
elif [ "$1" = attach ]; then
    for mount; do :; done
    mkdir -p "$mount/Ultimate-NAG52-Config-App.app/Contents/MacOS"
    touch "$mount/Ultimate-NAG52-Config-App.app/Contents/MacOS/config_app"
    chmod +x "$mount/Ultimate-NAG52-Config-App.app/Contents/MacOS/config_app"
fi''')
        self.tool("plutil", '''case "$*" in
    *CFBundleExecutable*) echo config_app;;
    *LSMinimumSystemVersion*) echo 11.0;;
esac''')
        self.tool("lipo", "echo arm64")
        self.tool("xcrun", "echo 'minos 11.0'")
        self.tool("otool", "printf 'config_app:\\n /usr/lib/libSystem.B.dylib\\n'")
        self.tool("osascript", '[ "$TEST_MODE" != desktop-fails ]')
        self.tool("open", "exit 1")
        return self.run_verifier("macos-dmg", self.root / "fixture.dmg", mode)

    def test_linux_environment_failures_allow_publication(self):
        for mode in ("server-fails", "gl-fails", "old-gl"):
            with self.subTest(mode=mode):
                result = self.linux(mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("::warning::", result.stdout)
                self.assertIn("NOT CHECKED", self.summary.read_text())

    def test_linux_application_failure_blocks_publication(self):
        result = self.linux("app-fails")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("::warning::", result.stdout)

    def test_linux_package_failure_blocks_even_without_environment(self):
        result = self.linux("bad-package")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("::warning::", result.stdout)

    def test_macos_environment_failure_allows_publication(self):
        result = self.macos("desktop-fails")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("::warning::", result.stdout)
        self.assertIn("NOT CHECKED", self.summary.read_text())

    def test_macos_application_failure_blocks_publication(self):
        result = self.macos("app-fails")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("::warning::", result.stdout)

    def test_macos_package_failure_blocks_publication(self):
        result = self.macos("bad-package")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("::warning::", result.stdout)


if __name__ == "__main__":
    unittest.main()
