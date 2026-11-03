"""Instantiate templates from the installed template catalog and build their projects."""

import json
import os
import re
import shutil
import subprocess
import sys
import uuid
from pathlib import Path
from xml.etree import ElementTree as ET


TEMPLATE = "{http://schemas.microsoft.com/developer/vstemplate/2005}"
CATALOG = "{http://schemas.microsoft.com/developer/vstemplatemanifest/2015}"
EXPECTED = {
    "Crust.Console": "executable",
    "Crust.StaticLibrary": "static-library",
    "Crust.SharedLibrary": "shared-library",
}


def run(arguments, env=None):
    result = subprocess.run(
        [str(arg) for arg in arguments], capture_output=True, text=True,
        encoding="utf-8", errors="replace", timeout=60, env=env,
    )
    if result.returncode:
        raise AssertionError(f"{arguments}\n{result.stdout}\n{result.stderr}")
    return result.stdout


def main():
    package, visual_studio = map(Path, sys.argv[1:])
    compiler = package / "Server/crust.exe"
    sdk = package / "Sdk"
    msbuild = visual_studio / "MSBuild/Current/Bin/MSBuild.exe"
    sources = Path(__file__).resolve().parents[1] / "Templates"
    catalog_root = package / "ProjectTemplates"
    entries = []
    for catalog in catalog_root.glob("*.vstman"):
        entries.extend(ET.parse(catalog).getroot().findall(CATALOG + "VSTemplateContainer"))
    assert len(entries) == len(EXPECTED), "VSIX must index all three templates for Visual Studio"
    seen = set()
    for entry in entries:
        assert entry.get("TemplateType") == "Project"
        relative = entry.findtext(CATALOG + "RelativePathOnDisk").replace("\\", "/")
        template_root = catalog_root / relative
        template_file = template_root / entry.findtext(CATALOG + "TemplateFileName")
        template = ET.parse(template_file).getroot()
        data = template.find(TEMPLATE + "TemplateData")
        identity = data.findtext(TEMPLATE + "TemplateID")
        assert identity in EXPECTED and identity not in seen, identity
        seen.add(identity)
        assert data.findtext(TEMPLATE + "ProjectType") == "Crust"
        assert data.findtext(TEMPLATE + "LanguageTag") == "Crust"
        assert entry.find(".//" + TEMPLATE + "TemplateID").text == identity
        # Check the catalog's extracted files, which Visual Studio actually opens.
        for path in template_root.rglob("*"):
            if path.is_file():
                original = sources / template_root.name / path.relative_to(template_root)
                assert path.read_bytes() == original.read_bytes(), str(path)

        name = template_root.name + " project \u00fc & spaces"
        destination = package / "template-tests" / name
        destination.mkdir(parents=True)
        replacements = {
            "$projectname$": name,
            "$safeprojectname$": re.sub(r"[^A-Za-z0-9_]", "_", name),
            "$guid1$": str(uuid.uuid4()),
        }

        def replace(value):
            for token, replacement in replacements.items():
                value = value.replace(token, replacement)
            return value

        def copy_file(node, source, target):
            target.parent.mkdir(parents=True, exist_ok=True)
            content = source.read_bytes()
            if node.get("ReplaceParameters", "false").lower() == "true":
                content = replace(content.decode("utf-8")).encode("utf-8")
            target.write_bytes(content)

        def copy_items(parent, source, target):
            for node in parent:
                if node.tag == TEMPLATE + "Folder":
                    copy_items(node, source / node.get("Name"), target / node.get("TargetFolderName", node.get("Name")))
                else:
                    assert node.tag == TEMPLATE + "ProjectItem", node.tag
                    copy_file(node, source / node.text, target / replace(node.get("TargetFileName", node.text)))

        project_node = template.find(TEMPLATE + "TemplateContent/" + TEMPLATE + "Project")
        project = destination / replace(project_node.get("TargetFileName"))
        copy_file(project_node, template_root / project_node.get("File"), project)
        copy_items(project_node, template_root, destination)
        ET.parse(project)
        # A checkout with only the manifest and sources must build independently
        # of Visual Studio, its SDK, and the optional MSBuild project file.
        portable = package / "portable-template-tests" / name
        portable.mkdir(parents=True)
        copy_items(project_node, template_root, portable)
        assert not list(portable.rglob("*.crproj"))
        sdk_settings = {"crust_sdk_path", "crustsdkpath", "crust_compiler_path", "crust_lsp_path"}
        standalone_env = {key: value for key, value in os.environ.items() if key.lower() not in sdk_settings}
        standalone_env["PATH"] = os.pathsep.join(
            directory for directory in standalone_env["PATH"].split(os.pathsep)
            if not any((Path(directory) / tool).is_file() for tool in ("MSBuild.exe", "dotnet.exe"))
        )
        assert shutil.which("MSBuild.exe", path=standalone_env["PATH"]) is None
        assert shutil.which("dotnet.exe", path=standalone_env["PATH"]) is None
        run([compiler, "check", "--project", portable], env=standalone_env)
        run([compiler, "build", "--project", portable], env=standalone_env)
        portable_target, = json.loads(run([compiler, "metadata", "--project", portable]))["targets"]
        assert Path(portable_target["output"]).is_file(), portable_target
        metadata = json.loads(run([compiler, "metadata", "--project", destination]))
        target, = metadata["targets"]
        assert target["kind"] == EXPECTED[identity] and target["debug"], metadata
        for host in ([msbuild], ["dotnet", "msbuild"]):
            run([*host, project, "-t:Build", "-v:quiet", "-nr:false", f"-p:CrustSdkPath={sdk}"])
        output = Path(target["output"])
        assert output.is_file(), output
        if target["kind"] == "executable":
            assert run([output]).strip() == "Hello from Crust!"
            assert run([portable_target["output"]]).strip() == "Hello from Crust!"
            assert output.with_suffix(".pdb").is_file(), "Console template must support source debugging"
        else:
            consumer = portable / "consumer"
            consumer.mkdir()
            (consumer / "crust.toml").write_text(
                '[project]\nname = "consumer"\n[dependencies]\napi = { path = ".." }\n'
                '[[targets]]\nname = "app"\nkind = "executable"\nentry = "main.cr"\n'
                '[targets.native]\nlink-targets = ["api:library"]\n', encoding="utf-8",
            )
            (consumer / "main.cr").write_text(
                'extern int add(int, int) = "crust_add";\nint main() { print(add(20, 22)); return 0; }\n',
                encoding="utf-8",
            )
            assert run([compiler, "run", "--project", consumer], env=standalone_env).strip() == "42"
        print(f"{identity}: packaged catalog, standalone CLI without .crproj, both MSBuild hosts, and execution passed")
    assert seen == set(EXPECTED)


if __name__ == "__main__":
    main()
