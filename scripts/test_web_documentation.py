"""Typecheck the library guide examples against built public declarations."""

import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "web"


class WebDocumentationTest(unittest.TestCase):
    def test_consumer_examples(self):
        """Require real package output; missing build prerequisites must fail."""
        packages = {
            "viewer": "markview", "editor": "editor", "fonts": "fonts",
            "resources": "resources", "scroll-sync": "scroll-sync",
        }
        paths = {
            f"@markview/{name}": [str(WEB / "packages" / folder / "dist/index.d.ts")]
            for name, folder in packages.items()
        }
        for declarations in paths.values():
            self.assertTrue(Path(declarations[0]).is_file(), "Run pnpm --dir web build first")
        with tempfile.TemporaryDirectory(prefix="markview-docs-") as directory:
            temporary = Path(directory)
            sources = []
            for page in sorted((ROOT / "docs/library").glob("*.md")):
                blocks = re.findall(r"^```ts\n(.*?)^```", page.read_text(), re.M | re.S)
                if blocks:
                    source = temporary / f"{page.stem}.ts"
                    source.write_text("\n".join(blocks))
                    sources.append(str(source))
            self.assertTrue(sources, "No consumer examples found")
            assets = temporary / "assets.d.ts"
            assets.write_text('declare module "*?url" { const url: string; export default url; }\n')
            config = temporary / "tsconfig.json"
            config.write_text(json.dumps({
                "compilerOptions": {
                    "target": "ES2022", "module": "ESNext",
                    "moduleResolution": "Bundler", "strict": True,
                    "noEmit": True, "skipLibCheck": False, "paths": paths,
                },
                "files": [*sources, str(assets)],
            }))
            subprocess.run([
                "node", str(WEB / "node_modules/typescript/bin/tsc"),
                "--project", str(config),
            ], check=True)


if __name__ == "__main__":
    unittest.main()
