vim.opt.rtp:append(vim.fn.getcwd() .. "/integrations/nvim")

local h = require("harnessd")
local fixture = vim.fn.tempname() .. ".rs"
vim.fn.writefile({ "fn demo() {", "    let value = 1;", "}" }, fixture)
vim.cmd("edit " .. vim.fn.fnameescape(fixture))
local source = vim.api.nvim_get_current_buf()
vim.api.nvim_win_set_cursor(0, { 2, 4 })

local requests = {}
local launches = {}
local notifications = {}
vim.notify = function(message, level)
  notifications[#notifications + 1] = { message = message, level = level }
end
vim.system = function(args, opts, callback)
  requests[#requests + 1] = { args = args, opts = opts }
  local method = args[2]
  local stdout
  if method == "inline" then
    stdout = vim.json.encode({
      result = { suggestion = { insert_text = "let added = true;\nreturn added;" } },
    })
  elseif method == "complete" then
    stdout = vim.json.encode({
      result = { suggestions = { { insert_text = "cached_value" } } },
    })
  elseif method == "scratch" then
    stdout = vim.json.encode({
      result = {
        path = vim.fn.getcwd() .. "/scratch/harnessd/demo.rs",
        relative_path = "scratch/harnessd/demo.rs",
        bytes = 42,
        lines = 3,
        created_at = 1,
        source_file = fixture,
        prompt_preview = "sketch a scratch file",
      },
    })
  elseif method == "thread" and args[3] == "create" then
    stdout = vim.json.encode({
      result = {
        thread = {
          thread_id = "thread-test",
          workspace = vim.fn.getcwd(),
          file = fixture,
          original_line = 2,
          current_line = 2,
          byte_offset = 16,
          line_hash = "hash",
          line_preview = "let unsaved_value = 1;",
          prompt_preview = "open a thread",
          prompt = "open a thread",
          status = "open",
          created_at = 1,
          updated_at = 1,
        },
        launch = {
          argv = { "codex", "--no-alt-screen", "-C", vim.fn.getcwd(), "open a thread" },
          cwd = vim.fn.getcwd(),
          started_after_unix = 1,
        },
      },
    })
  elseif method == "thread" and args[3] == "list" then
    stdout = vim.json.encode({
      result = {
        threads = {
          {
            thread_id = "thread-test",
            workspace = vim.fn.getcwd(),
            file = fixture,
            original_line = 2,
            current_line = 2,
            byte_offset = 16,
            line_hash = "hash",
            line_preview = "let unsaved_value = 1;",
            prompt_preview = "open a thread",
            prompt = "open a thread",
            status = "open",
            created_at = 1,
            updated_at = 1,
          },
        },
      },
    })
  elseif method == "thread" and args[3] == "resolve" then
    stdout = vim.json.encode({ result = { resolved = false } })
  elseif method == "codex-sessions" then
    stdout = vim.json.encode({
      result = {
        sessions = {
          {
            id = "session-test",
            path = "/tmp/session.jsonl",
            cwd = vim.fn.getcwd(),
            timestamp = "2026-05-29T00:00:00Z",
            preview = "saved session",
            modified_at = 1,
            project_match = true,
          },
        },
      },
    })
  else
    stdout = vim.json.encode({ result = {} })
  end
  callback({ code = 0, stdout = stdout, stderr = "" })
  return {}
end

h.setup({
  command = "harnessd",
  terminal_launcher = function(argv, opts, bufnr)
    launches[#launches + 1] = { argv = argv, opts = opts, bufnr = bufnr }
  end,
})
assert(vim.fn.exists(":HarnessdAsk") == 2)
assert(vim.fn.exists(":HarnessdInline") == 2)
assert(vim.fn.exists(":HarnessdScratch") == 2)
assert(vim.fn.exists(":HarnessdThreads") == 2)
assert(vim.fn.exists(":HarnessdAccept") == 2)
assert(vim.fn.maparg("<Plug>(HarnessdAccept)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdScratch)", "n") ~= "")

h.inline_ask()
assert(#vim.api.nvim_list_wins() == 2, "ask should create a floating prompt window")
h.dismiss()
assert(#vim.api.nvim_list_wins() == 1, "dismiss should close the prompt")

vim.api.nvim_buf_set_lines(source, 1, 2, false, { "    let unsaved_value = 1;" })
h.inline_ask()
local prompt_bufnr = vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_lines(prompt_bufnr, 0, -1, false, { "insert output" })
local submitted = false
for _, mapping in ipairs(vim.api.nvim_buf_get_keymap(prompt_bufnr, "i")) do
  if mapping.lhs == "<CR>" then
    mapping.callback()
    submitted = true
  end
end
assert(submitted, "prompt should install a submit mapping")
local ns = vim.api.nvim_get_namespaces().harnessd_preview
vim.wait(1000, function()
  return #vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 1
end)
assert(requests[#requests].args[2] == "inline")
assert(requests[#requests].opts.stdin:find("unsaved_value", 1, true))
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 1)
h.accept()
local lines = vim.api.nvim_buf_get_lines(source, 1, 3, false)
assert(table.concat(lines, "\n"):find("let added = true;\nreturn added;", 1, true))

vim.bo[source].modified = false
local complete_done = false
h.preview_complete({}, function(_, err)
  assert(err == nil)
  complete_done = true
end)
vim.wait(1000, function() return complete_done end)
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 1)
h.accept()
assert(vim.api.nvim_get_current_line():find("cached_value", 1, true))
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 0)

vim.bo[source].modified = false
local second_done = false
h.preview_complete({}, function(_, err)
  assert(err == nil)
  second_done = true
end)
vim.wait(1000, function() return second_done end)
vim.api.nvim_buf_set_lines(source, 0, 1, false, { "fn changed() {" })
local before = table.concat(vim.api.nvim_buf_get_lines(source, 0, -1, false), "\n")
h.accept()
local after = table.concat(vim.api.nvim_buf_get_lines(source, 0, -1, false), "\n")
assert(after == before, "stale preview must not insert")
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 0)

vim.api.nvim_set_current_buf(source)
local scratch_before = table.concat(vim.api.nvim_buf_get_lines(source, 0, -1, false), "\n")
h.scratch_ask()
local scratch_prompt = vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_lines(scratch_prompt, 0, -1, false, { "sketch a scratch file" })
local scratch_submitted = false
for _, mapping in ipairs(vim.api.nvim_buf_get_keymap(scratch_prompt, "i")) do
  if mapping.lhs == "<CR>" then
    mapping.callback()
    scratch_submitted = true
  end
end
assert(scratch_submitted, "scratch prompt should install a submit mapping")
vim.wait(1000, function()
  return requests[#requests] and requests[#requests].args[2] == "scratch"
end)
assert(requests[#requests].opts.stdin:find("fn changed", 1, true))
assert(table.concat(vim.api.nvim_buf_get_lines(source, 0, -1, false), "\n") == scratch_before)
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 0)
vim.wait(1000, function()
  for _, notification in ipairs(notifications) do
    if notification.message == "harnessd scratch: scratch/harnessd/demo.rs" then
      return true
    end
  end
  return false
end)
local scratch_notified = false
for _, notification in ipairs(notifications) do
  if notification.message == "harnessd scratch: scratch/harnessd/demo.rs" then
    scratch_notified = true
  end
end
assert(scratch_notified, "scratch should notify the created relative path")

vim.api.nvim_set_current_buf(source)
h.thread_ask()
local thread_prompt = vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_lines(thread_prompt, 0, -1, false, { "open a thread" })
local thread_submitted = false
for _, mapping in ipairs(vim.api.nvim_buf_get_keymap(thread_prompt, "i")) do
  if mapping.lhs == "<CR>" then
    mapping.callback()
    thread_submitted = true
  end
end
assert(thread_submitted, "thread prompt should install a submit mapping")
vim.wait(1000, function()
  return #launches == 1
end)
assert(launches[1].argv[1] == "codex")
local thread_ns = vim.api.nvim_get_namespaces().harnessd_threads
assert(#vim.api.nvim_buf_get_extmarks(source, thread_ns, 0, -1, {}) >= 1)

vim.fn.delete(fixture)
print("harnessd nvim headless tests passed")
