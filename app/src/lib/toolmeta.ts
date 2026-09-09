import data from "@/data/tools.json"

export type Risk = "read" | "write" | "exec"

/** 执行类:可在本机跑代码/命令 —— 默认关闭 */
const EXEC = new Set([
  "execute_terminal_command",
  "runNotebookCell",
  "run_inspection_kts",
  "execute_sql_query",
  "execute_run_configuration",
  "build_project",
])

/** 写入类:会修改文件/IDE 状态 —— 默认关闭 */
const WRITE = new Set([
  "create_new_file",
  "replace_text_in_file",
  "rename_refactoring",
  "reformat_file",
  "open_file_in_editor",
])

/** 默认全部放开(含写入/执行);需要白名单模式时改为 false 并维护白名单集合 */
export const ALLOW_ALL = true

/** 常用工具的中文一句话说明(没有则用英文描述首行截断) */
const ZH: Record<string, string> = {
  read_file: "读取项目文件或依赖源码",
  get_file_text_by_path: "按路径读取文件内容",
  list_directory_tree: "列出目录树",
  find_files_by_glob: "按 glob 模式查找文件",
  find_files_by_name_keyword: "按名称关键字查找文件",
  search_regex: "正则搜索项目文件",
  search_text: "全文搜索项目文件",
  search_symbol: "按名称查找符号",
  search_file: "搜索文件",
  get_symbol_info: "符号文档/签名信息",
  get_file_problems: "文件错误与警告(IDE 检查)",
  get_project_modules: "列出项目模块",
  get_project_dependencies: "列出项目依赖",
  get_run_configurations: "列出运行配置",
  get_all_open_file_paths: "编辑器中打开的文件",
  get_repositories: "列出 VCS 仓库",
  rename_refactoring: "重命名重构",
  replace_text_in_file: "查找并替换文件内容",
  create_new_file: "新建文件",
  reformat_file: "格式化文件",
  open_file_in_editor: "在编辑器中打开文件",
  build_project: "构建项目并返回错误",
  execute_run_configuration: "执行运行配置",
  execute_terminal_command: "在 IDE 终端执行 shell 命令",
  runNotebookCell: "执行 Jupyter 单元格",
  run_inspection_kts: "运行 Kotlin 检查脚本",
  generate_psi_tree: "生成 PSI 语法树",
  execute_sql_query: "执行 SQL 查询",
  preview_table_data: "预览表数据",
}

export interface ToolInfo {
  name: string
  desc: string
  risk: Risk
  enabled: boolean
}

export function riskOf(name: string): Risk {
  if (EXEC.has(name)) return "exec"
  if (WRITE.has(name)) return "write"
  return "read"
}

export const TOOLS: ToolInfo[] = data.tools.map((t) => ({
  name: t.name,
  desc:
    ZH[t.name] ??
    (t.description ?? "").replace(/\s+/g, " ").trim().slice(0, 60),
  risk: riskOf(t.name),
  enabled: ALLOW_ALL,
}))

export const SERVER = data.server
export const TOOL_COUNT = TOOLS.length
export const ENABLED_COUNT = TOOLS.filter((t) => t.enabled).length
