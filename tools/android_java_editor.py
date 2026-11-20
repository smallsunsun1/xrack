"""Prepare the VS Code Java classpath from Bazel's Android inputs."""

from contextlib import suppress
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory


ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "crates/xrack-platform-winit/android"
TARGET = "editor_classpath"
JARS = ("android.jar", "activity-resources.jar")

PROJECT = r"""<?xml version="1.0" encoding="UTF-8"?>
<projectDescription>
    <name>XRack Android Java</name>
    <buildSpec>
        <buildCommand>
            <name>org.eclipse.jdt.core.javabuilder</name>
        </buildCommand>
    </buildSpec>
    <natures>
        <nature>org.eclipse.jdt.core.javanature</nature>
    </natures>
    <filteredResources>
        <filter>
            <id>1</id>
            <name/>
            <type>26</type>
            <matcher>
                <id>org.eclipse.core.resources.regexFilterMatcher</id>
                <arguments>\.git|bazel-.*|node_modules</arguments>
            </matcher>
        </filter>
        <filter>
            <id>2</id>
            <name>.tools</name>
            <type>14</type>
            <matcher>
                <id>org.eclipse.core.resources.regexFilterMatcher</id>
                <arguments>^(?!android-java$).+$</arguments>
            </matcher>
        </filter>
    </filteredResources>
</projectDescription>
"""

CLASSPATH = """<?xml version="1.0" encoding="UTF-8"?>
<classpath>
    <classpathentry kind="src" path="crates/xrack-platform-winit/android"/>
    <classpathentry kind="src" path="examples/android/testing"/>
    <classpathentry kind="con" path="org.eclipse.jdt.launching.JRE_CONTAINER"/>
    <classpathentry kind="lib" path=".tools/android-java/android.jar"/>
    <classpathentry kind="lib" path=".tools/android-java/activity-resources.jar"/>
    <classpathentry kind="output" path=".tools/android-java/classes"/>
</classpath>
"""


def prepare_project(root: Path) -> None:
    for name, content in ((".project", PROJECT), (".classpath", CLASSPATH)):
        with suppress(FileExistsError):
            with (root / name).open("x", encoding="utf-8") as project_file:
                project_file.write(content)


def main() -> None:
    subprocess.run(
        ["bazel", "build", "--config=android", f"//{PACKAGE}:{TARGET}"],
        cwd=ROOT,
        check=True,
    )
    result = subprocess.run(
        ["bazel", "info", "--config=android", "bazel-bin"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    source = Path(result.stdout.strip()) / PACKAGE / TARGET
    destination = ROOT / ".tools/android-java"
    destination.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(dir=destination) as staging:
        for name in JARS:
            shutil.copyfile(source / name, Path(staging) / name)
        for name in JARS:
            (Path(staging) / name).replace(destination / name)
    prepare_project(ROOT)
    print(f"Prepared Java classpath in {destination.relative_to(ROOT)}.")


if __name__ == "__main__":
    main()
