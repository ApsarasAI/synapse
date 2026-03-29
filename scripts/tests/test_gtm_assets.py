import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
DEMO_SCRIPT = REPO_ROOT / "docs" / "product" / "demo-script.md"
OBJECTIONS_LOG = REPO_ROOT / "docs" / "product" / "objections-log-template.md"
POC_PLAYBOOK = REPO_ROOT / "docs" / "product" / "poc-playbook.md"


class GtmAssetTests(unittest.TestCase):
    def test_demo_script_covers_required_sections(self):
        content = DEMO_SCRIPT.read_text()

        for expected in (
            "# Standard Demo Script",
            "## 3. 开场话术",
            "## 5. 演示流程",
            "### 步骤三：展示标准 demo",
            "## 7. 演示后记录",
            "docs/product/objections-log-template.md",
        ):
            self.assertIn(expected, content)

    def test_objections_log_template_covers_required_fields(self):
        content = OBJECTIONS_LOG.read_text()

        for expected in (
            "# Objections Log Template",
            "| Date | 记录日期 |",
            "| Objection | 客户提出的问题或阻塞 |",
            "| Artifact Gap | 当前缺少的文档、测试、功能或材料 |",
            "## 5. 周度复盘输出",
        ):
            self.assertIn(expected, content)

    def test_poc_playbook_mentions_objections_output(self):
        content = POC_PLAYBOOK.read_text()
        self.assertIn("objections 列表", content)


if __name__ == "__main__":
    unittest.main()
