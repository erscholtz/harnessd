vim.opt.rtp:append(vim.fn.getcwd() .. "/integrations/nvim")

local h = require("harnessd")
local fixture = vim.fn.tempname() .. ".rs"
vim.fn.writefile({ "fn demo() {", "    let value = 1;", "}" }, fixture)
vim.fn.mkdir(vim.fn.getcwd() .. "/scratch/hash/thread-test", "p")
vim.fn.writefile({ "// harnessd scratch preview", "fn main() {}" }, vim.fn.getcwd() .. "/scratch/hash/thread-test/demo.rs")
vim.cmd("edit " .. vim.fn.fnameescape(fixture))
local source = vim.api.nvim_get_current_buf()
vim.api.nvim_win_set_cursor(0, { 2, 4 })

local requests = {}
local launches = {}
local notifications = {}
local channel_sends = {}
local mock_prepare_error = nil
local mock_inline_fast_mode = "suggestion"
local mock_delay_inline_fast = false
local pending_inline_fast = nil
local function has_arg_pair(args, first, second)
  for index = 1, #args - 1 do
    if args[index] == first and args[index + 1] == second then
      return true
    end
  end
  return false
end
local function find_request(method, submethod)
  for index = #requests, 1, -1 do
    local args = requests[index].args
    if args[2] == method and (submethod == nil or args[3] == submethod) then
      return requests[index]
    end
  end
  return nil
end
vim.notify = function(message, level)
  notifications[#notifications + 1] = { message = message, level = level }
end
vim.api.nvim_chan_send = function(channel, data)
  channel_sends[#channel_sends + 1] = { channel = channel, data = data }
end
vim.system = function(args, opts, callback)
  requests[#requests + 1] = { args = args, opts = opts }
  local method = args[2]
  local stdout
  if method == "inline" then
    stdout = vim.json.encode({
      result = { suggestion = { insert_text = "let added = true;\nreturn added;" } },
    })
  elseif method == "bridge" and args[4] == "inline.fast" then
    if mock_inline_fast_mode == "refresh" then
      stdout = vim.json.encode({
        result = { source = "none", refresh_queued = true },
      })
    elseif mock_inline_fast_mode == "error" then
      stdout = vim.json.encode({
        error = { code = -32001, message = "inline fast unavailable" },
      })
    else
      stdout = vim.json.encode({
        result = { suggestion = { insert_text = "let added = true;\nreturn added;" }, source = "cache", refresh_queued = false },
      })
    end
    if mock_delay_inline_fast then
      pending_inline_fast = function()
        callback({ code = 0, stdout = stdout, stderr = "" })
      end
      return
    end
  elseif method == "bridge" and args[4] == "inline.prepare" then
    if mock_prepare_error then
      stdout = vim.json.encode({
        error = { code = -32001, message = mock_prepare_error },
      })
    else
      stdout = vim.json.encode({
        result = { prepared = true },
      })
    end
  elseif method == "complete" then
    stdout = vim.json.encode({
      result = { suggestions = { { insert_text = "cached_value" } } },
    })
  elseif method == "scratch" then
    stdout = vim.json.encode({
      result = {
        path = vim.fn.getcwd() .. "/scratch/hash/standalone/demo.rs",
        relative_path = "scratch/hash/standalone/demo.rs",
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
            mark_id = "mark-test",
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
          argv = { "codex", "--no-alt-screen", "--model", "gpt-5.5", "-c", "model_reasoning_effort=\"high\"", "-C", vim.fn.getcwd(), "open a thread" },
          cwd = vim.fn.getcwd(),
          started_after_unix = 1,
          model = "gpt-5.5",
          reasoning_effort = "high",
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
            examples = {
              {
                example_id = "example-test",
                thread_id = "thread-test",
                title = "show usage",
                path = vim.fn.getcwd() .. "/scratch/hash/thread-test/demo.rs",
                relative_path = "scratch/hash/thread-test/demo.rs",
                prompt = "show usage",
                prompt_preview = "show usage",
                source_file = fixture,
                bytes = 42,
                lines = 3,
                created_at = 2,
              },
            },
            created_at = 1,
            updated_at = 1,
          },
        },
      },
    })
  elseif method == "thread" and args[3] == "example" then
    stdout = vim.json.encode({
      result = {
          thread = {
            thread_id = "thread-test",
            mark_id = "mark-test",
          workspace = vim.fn.getcwd(),
          file = fixture,
          original_line = 2,
          current_line = 2,
          byte_offset = 16,
          line_hash = "hash",
          line_preview = "let unsaved_value = 1;",
          prompt_preview = "open a thread",
          prompt = "open a thread",
          status = "linked",
          examples = {
            {
              example_id = "example-test",
              thread_id = "thread-test",
              title = "show usage",
              path = vim.fn.getcwd() .. "/scratch/hash/thread-test/demo.rs",
              relative_path = "scratch/hash/thread-test/demo.rs",
              prompt = "show usage",
              prompt_preview = "show usage",
              source_file = fixture,
              bytes = 42,
              lines = 3,
              created_at = 2,
            },
          },
          created_at = 1,
          updated_at = 2,
        },
        example = {
          example_id = "example-test",
          thread_id = "thread-test",
          title = "show usage",
          path = vim.fn.getcwd() .. "/scratch/hash/thread-test/demo.rs",
          relative_path = "scratch/hash/thread-test/demo.rs",
          prompt = "show usage",
          prompt_preview = "show usage",
          source_file = fixture,
          bytes = 42,
          lines = 3,
          created_at = 2,
        },
      },
    })
  elseif method == "thread" and args[3] == "resolve" then
    stdout = vim.json.encode({ result = { resolved = false } })
  elseif method == "mark" and args[3] == "list" then
    stdout = vim.json.encode({
      result = {
        marks = {
          {
            mark_id = "mark-test",
            workspace = vim.fn.getcwd(),
            file = fixture,
            original_line = 2,
            current_line = 2,
            byte_offset = 16,
            line_hash = "hash",
            line_preview = "let unsaved_value = 1;",
            thread_id = "thread-test",
            status = "linked",
            created_at = 1,
            updated_at = 1,
          },
        },
      },
    })
  elseif method == "mark" and (args[3] == "next" or args[3] == "prev") then
    stdout = vim.json.encode({
      result = {
        mark = {
          mark_id = "mark-test",
          workspace = vim.fn.getcwd(),
          file = fixture,
          original_line = 2,
          current_line = 2,
          byte_offset = 16,
          line_hash = "hash",
          line_preview = "let unsaved_value = 1;",
          thread_id = "thread-test",
          status = "linked",
          created_at = 1,
          updated_at = 1,
        },
      },
    })
  elseif method == "settings" and args[3] == "get" then
    stdout = vim.json.encode({
      result = { settings = { scratch_storage_mode = "runtime", read_scope = "current_context" } },
    })
  elseif method == "settings" and args[3] == "update" then
    local storage = "runtime"
    for index = 1, #args - 1 do
      if args[index] == "--scratch-storage-mode" then
        storage = args[index + 1]
      end
    end
    stdout = vim.json.encode({
      result = { settings = { scratch_storage_mode = storage, read_scope = "current_context" } },
    })
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
    return 77
  end,
})
assert(vim.fn.exists(":HarnessdPanel") == 2)
assert(vim.fn.exists(":HarnessdPanelFlip") == 2)
assert(vim.fn.exists(":HarnessdExample") == 2)
assert(vim.fn.exists(":HarnessdAsk") == 2)
assert(vim.fn.exists(":HarnessdScratch") == 2)
assert(vim.fn.exists(":HarnessdThreads") == 2)
assert(vim.fn.exists(":HarnessdMarks") == 2)
assert(vim.fn.exists(":HarnessdMarkNext") == 2)
assert(vim.fn.exists(":HarnessdMarkPrev") == 2)
assert(vim.fn.exists(":HarnessdSettings") == 2)
assert(vim.fn.exists(":HarnessdModels") == 2)
assert(vim.fn.exists(":HarnessdInline") == 0)
assert(vim.fn.exists(":HarnessdInlineComplete") == 0)
assert(vim.fn.exists(":HarnessdPrepareContext") == 0)
assert(vim.fn.exists(":HarnessdAccept") == 2)
assert(vim.fn.maparg("<Plug>(HarnessdPanel)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdExample)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdAccept)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdAcceptLine)", "i") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdInlineComplete)", "i") == "")
assert(vim.fn.maparg("<Plug>(HarnessdPrepareContext)", "i") == "")
assert(vim.fn.maparg("<Plug>(HarnessdScratch)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdMarks)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdMarkNext)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdMarkPrev)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdSettings)", "n") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdModels)", "n") ~= "")
assert(h.model_for("line", source) == "gpt-5.4-mini")
assert(h.reasoning_effort_for("line", source) == "low")
assert(h.model_for("ask", source) == nil)
assert(h.model_for("scratch", source) == nil)
assert(h.set_model("ask", { model = "gpt-5.5", reasoning_effort = "high" }, source) == "gpt-5.5")
assert(h.set_model("scratch", { model = "gpt-5.4-mini", reasoning_effort = "low" }, source) == "gpt-5.4-mini")
assert(h.get_models(source).ask.model == "gpt-5.5")
assert(h.get_models(source).ask.reasoning_effort == "high")
assert(h.is_auto_inline_enabled() == false)
assert(h.statusline():find("harnessd: %[closed%]"))
assert(h.toggle_auto_inline() == true)
assert(h.is_auto_inline_enabled() == true)
assert(h.statusline():find("harnessd: %[closed%]"))
assert(h.toggle_auto_inline() == false)
assert(h.statusline():find("harnessd: %[closed%]"))

local settings_done = false
h.settings_update({ scratch_storage_mode = "temp" }, function(_, err)
  assert(err == nil)
  settings_done = true
end)
vim.wait(1000, function() return settings_done end)
assert(requests[#requests].args[2] == "settings")
assert(requests[#requests].args[3] == "update")
assert(has_arg_pair(requests[#requests].args, "--scratch-storage-mode", "temp"))

local prepare_done = false
h.prepare_inline({ force = true }, function(_, err)
  assert(err == nil)
  prepare_done = true
end)
assert(h.context_status().state == "loading")
assert(h.context_status().message == "preparing")
assert(h.statusline():find("harnessd:"))
vim.wait(1000, function() return prepare_done end)
assert(requests[#requests].args[2] == "bridge")
assert(requests[#requests].args[4] == "inline.prepare")
assert(has_arg_pair(requests[#requests].args, "--model", "gpt-5.4-mini"))
assert(has_arg_pair(requests[#requests].args, "--reasoning-effort", "low"))
assert(h.context_status().state == "ready")
assert(h.context_status().last_attempt_at ~= nil)
assert(h.context_status().last_ready_at ~= nil)
assert(h.statusline():find("harnessd: %[closed%]"))

mock_prepare_error = "context fetch failed"
local prepare_failed = false
h.prepare_inline({ force = true }, function(_, err)
  assert(err:find("context fetch failed", 1, true))
  prepare_failed = true
end)
vim.wait(1000, function() return prepare_failed end)
assert(h.context_status().state == "failed")
assert(h.context_status().message:find("context fetch failed", 1, true))
assert(h.statusline():find("harnessd:"))
mock_prepare_error = nil

mock_delay_inline_fast = true
local delayed_done = false
h.inline_complete({}, function(_, err)
  assert(err == nil)
  delayed_done = true
end)
assert(h.inline_status().state == "waiting")
assert(h.statusline():find("harnessd:"))
pending_inline_fast()
pending_inline_fast = nil
mock_delay_inline_fast = false
vim.wait(1000, function() return delayed_done end)
assert(h.inline_status().state == "idle")
assert(h.inline_status().source == "cache")
assert(h.statusline():find("harnessd:"))

mock_inline_fast_mode = "refresh"
local refresh_done = false
h.inline_complete({}, function(_, err)
  assert(err == nil)
  refresh_done = true
end)
vim.wait(1000, function() return refresh_done end)
assert(h.inline_status().refresh == "queued")
assert(h.statusline():find("harnessd:"))
mock_inline_fast_mode = "error"
local failed_done = false
h.inline_complete({}, function(_, err)
  assert(err:find("inline fast unavailable", 1, true))
  failed_done = true
end)
vim.wait(1000, function() return failed_done end)
assert(h.inline_status().state == "failed")
assert(h.inline_status().last_error:find("inline fast unavailable", 1, true))
assert(h.statusline():find("harnessd:"))
mock_inline_fast_mode = "suggestion"

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
assert(has_arg_pair(requests[#requests].args, "--model", "gpt-5.4-mini"))
assert(has_arg_pair(requests[#requests].args, "--reasoning-effort", "low"))
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
vim.api.nvim_win_set_cursor(0, { 2, 4 })
local inline_complete_done = false
h.inline_complete({ prompt = "complete the line" }, function(_, err)
  assert(err == nil)
  inline_complete_done = true
end)
vim.wait(1000, function() return inline_complete_done end)
assert(requests[#requests].args[2] == "bridge")
assert(has_arg_pair(requests[#requests].args, "--method", "inline.fast"))
assert(has_arg_pair(requests[#requests].args, "--model", "gpt-5.4-mini"))
assert(has_arg_pair(requests[#requests].args, "--reasoning-effort", "low"))
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 1)
h.accept_line()
local line_only = table.concat(vim.api.nvim_buf_get_lines(source, 1, 2, false), "\n")
assert(line_only:find("let added = true;", 1, true))
assert(not line_only:find("return added;", 1, true))
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
assert(has_arg_pair(requests[#requests].args, "--model", "gpt-5.4-mini"))
assert(has_arg_pair(requests[#requests].args, "--reasoning-effort", "low"))
assert(table.concat(vim.api.nvim_buf_get_lines(source, 0, -1, false), "\n") == scratch_before)
assert(#vim.api.nvim_buf_get_extmarks(source, ns, 0, -1, {}) == 0)
vim.wait(1000, function()
  for _, notification in ipairs(notifications) do
    if notification.message == "harnessd scratch: scratch/hash/standalone/demo.rs" then
      return true
    end
  end
  return false
end)
local scratch_notified = false
for _, notification in ipairs(notifications) do
  if notification.message == "harnessd scratch: scratch/hash/standalone/demo.rs" then
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
local thread_create_request = find_request("thread", "create")
assert(thread_create_request ~= nil)
assert(has_arg_pair(thread_create_request.args, "--model", "gpt-5.5"))
assert(has_arg_pair(thread_create_request.args, "--reasoning-effort", "high"))
assert(launches[1].argv[1] == "codex")
assert(has_arg_pair(launches[1].argv, "--model", "gpt-5.5"))
assert(has_arg_pair(launches[1].argv, "-c", "model_reasoning_effort=\"high\""))
h.send_model_to_active_thread("gpt-5.5")
assert(channel_sends[#channel_sends].channel == 77)
assert(channel_sends[#channel_sends].data == "/model gpt-5.5\n")
local thread_ns = vim.api.nvim_get_namespaces().harnessd_threads
assert(#vim.api.nvim_buf_get_extmarks(source, thread_ns, 0, -1, {}) >= 1)
local mark_ns = vim.api.nvim_get_namespaces().harnessd_marks
h.refresh_marks(source)
vim.wait(1000, function()
  return #vim.api.nvim_buf_get_extmarks(source, mark_ns, 0, -1, {}) >= 1
end)
assert(#vim.api.nvim_buf_get_extmarks(source, mark_ns, 0, -1, {}) >= 1)

local mark_jump_done = false
vim.api.nvim_set_current_buf(source)
h.mark_next_current()
vim.wait(1000, function()
  if find_request("mark", "next") ~= nil then
    mark_jump_done = true
  end
  return mark_jump_done
end)
assert(mark_jump_done, "mark next should request the daemon")

vim.api.nvim_set_current_buf(source)
vim.api.nvim_win_set_cursor(0, { 2, 4 })
h.panel()
assert(h.statusline():find("harnessd:"))
h.panel_flip()
assert(h.statusline():find("browse") or h.statusline():find("examples"))

h.example_ask()
local example_prompt = vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_lines(example_prompt, 0, -1, false, { "show usage" })
local example_submitted = false
for _, mapping in ipairs(vim.api.nvim_buf_get_keymap(example_prompt, "i")) do
  if mapping.lhs == "<CR>" then
    mapping.callback()
    example_submitted = true
  end
end
assert(example_submitted, "example prompt should install a submit mapping")
vim.wait(1000, function()
  return requests[#requests] and requests[#requests].args[2] == "thread" and requests[#requests].args[3] == "example"
end)
assert(has_arg_pair(requests[#requests].args, "--thread-id", "thread-test"))
assert(requests[#requests].opts.stdin:find("fn changed", 1, true))
vim.wait(1000, function()
  for _, notification in ipairs(notifications) do
    if notification.message == "harnessd example: scratch/hash/thread-test/demo.rs" then
      return true
    end
  end
  return false
end)
local example_notified = false
for _, notification in ipairs(notifications) do
  if notification.message == "harnessd example: scratch/hash/thread-test/demo.rs" then
    example_notified = true
  end
end
assert(example_notified, "example should notify the linked relative path")

h.setup({ legacy_autocomplete = true })
assert(vim.fn.exists(":HarnessdInline") == 2)
assert(vim.fn.exists(":HarnessdInlineComplete") == 2)
assert(vim.fn.exists(":HarnessdPrepareContext") == 2)
assert(vim.fn.maparg("<Plug>(HarnessdInlineComplete)", "i") ~= "")
assert(vim.fn.maparg("<Plug>(HarnessdPrepareContext)", "i") ~= "")

vim.fn.delete(fixture)
print("harnessd nvim headless tests passed")
vim.cmd("qa!")
