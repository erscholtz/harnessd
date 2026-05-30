local M = {}

local config = {
  command = "harnessd",
  codex_command = "codex",
  sidebar_width = 72,
  session_limit = 50,
  thread_sign_text = "H",
}

local namespace = vim.api.nvim_create_namespace("harnessd_preview")
local thread_namespace = vim.api.nvim_create_namespace("harnessd_threads")
local preview = nil
local prompt = nil
local sidebar = nil
local thread_sign_group = "harnessd_threads"
local thread_sign_id = 1000
local current_threads = {}

local function merge_config(opts)
  config = vim.tbl_deep_extend("force", config, opts or {})
end

local function decode_response(output)
  if not output or output == "" then
    return nil, "empty response from harnessd"
  end

  local ok, decoded = pcall(vim.json.decode, output)
  if not ok then
    return nil, decoded
  end

  if decoded.error then
    return nil, decoded.error.message or "harnessd request failed"
  end

  return decoded, nil
end

local function current_file(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  local path = vim.api.nvim_buf_get_name(bufnr)
  if path == "" then
    return nil, "buffer has no file path"
  end

  return path, nil
end

local function position(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  local cursor = vim.api.nvim_win_get_cursor(0)
  local row = cursor[1] - 1
  local col = cursor[2]
  local line_offset = vim.api.nvim_buf_get_offset(bufnr, row)

  if line_offset < 0 then
    return nil, "could not resolve buffer byte offset"
  end

  return {
    row = row,
    col = col,
    offset = line_offset + col,
  }, nil
end

local function buffer_content(bufnr)
  local content = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, true), "\n")
  if vim.bo[bufnr].endofline then
    content = content .. "\n"
  end
  return content
end

local function workspace()
  return vim.fn.getcwd()
end

local function sign_define()
  pcall(vim.fn.sign_define, "HarnessdThread", {
    text = config.thread_sign_text,
    texthl = "DiagnosticInfo",
    numhl = "DiagnosticInfo",
  })
  pcall(vim.fn.sign_define, "HarnessdThreadStale", {
    text = config.thread_sign_text,
    texthl = "DiagnosticWarn",
    numhl = "DiagnosticWarn",
  })
end

local function run_command(args, options, callback)
  if vim.system == nil then
    callback(nil, "vim.system() is required (Neovim 0.10+)")
    return
  end

  options = vim.tbl_extend("force", { text = true }, options or {})
  vim.system(args, options, function(result)
    vim.schedule(function()
      if result.code ~= 0 then
        local stderr = (result.stderr or ""):gsub("%s+$", "")
        callback(nil, stderr ~= "" and stderr or ("harnessd exited with code " .. result.code))
        return
      end

      local response, err = decode_response(result.stdout)
      callback(response, err)
    end)
  end)
end

local function close_prompt()
  if not prompt then
    return
  end
  if vim.api.nvim_win_is_valid(prompt.winid) then
    vim.api.nvim_win_close(prompt.winid, true)
  end
  if vim.api.nvim_buf_is_valid(prompt.bufnr) then
    vim.api.nvim_buf_delete(prompt.bufnr, { force = true })
  end
  prompt = nil
end

local function clear_preview()
  if preview and vim.api.nvim_buf_is_valid(preview.bufnr) then
    vim.api.nvim_buf_del_extmark(preview.bufnr, namespace, preview.extmark_id)
  end
  preview = nil
end

function M.dismiss()
  clear_preview()
  close_prompt()
end

local function render_preview(bufnr, cursor, insert_text, source)
  clear_preview()
  if insert_text == nil or insert_text == "" or not vim.api.nvim_buf_is_valid(bufnr) then
    return
  end

  local lines = vim.split(insert_text, "\n", { plain = true })
  local opts = {
    virt_text = { { lines[1], "Comment" } },
    virt_text_pos = "inline",
  }
  if #lines > 1 then
    opts.virt_lines = {}
    for index = 2, #lines do
      opts.virt_lines[#opts.virt_lines + 1] = { { lines[index], "Comment" } }
    end
  end

  local extmark_id = vim.api.nvim_buf_set_extmark(bufnr, namespace, cursor.row, cursor.col, opts)
  preview = {
    bufnr = bufnr,
    extmark_id = extmark_id,
    changedtick = vim.api.nvim_buf_get_changedtick(bufnr),
    row = cursor.row,
    col = cursor.col,
    insert_text = insert_text,
    source = source,
  }
end

function M.complete(opts, callback)
  opts = opts or {}
  callback = callback or function() end

  local file = opts.file or select(1, current_file(opts.bufnr))
  if not file then
    callback(nil, "file path is required")
    return
  end

  local offset = opts.offset
  if offset == nil then
    local current = select(1, position(opts.bufnr))
    offset = current and current.offset or nil
  end
  if offset == nil then
    callback(nil, "byte offset is required")
    return
  end

  local args = {
    config.command,
    "complete",
    "--file",
    file,
    "--offset",
    tostring(offset),
  }

  if opts.prefix and opts.prefix ~= "" then
    table.insert(args, "--prefix")
    table.insert(args, opts.prefix)
  end

  run_command(args, nil, callback)
end

function M.inline(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  local file = opts.file or select(1, current_file(bufnr))
  local prompt_text = opts.prompt
  if not file then
    callback(nil, "file path is required")
    return
  end
  if not prompt_text or prompt_text:match("^%s*$") then
    callback(nil, "inline prompt must not be empty")
    return
  end

  local offset = opts.offset
  if offset == nil then
    local current = select(1, position(bufnr))
    offset = current and current.offset or nil
  end
  if offset == nil then
    callback(nil, "byte offset is required")
    return
  end

  run_command({
    config.command,
    "inline",
    "--file",
    file,
    "--offset",
    tostring(offset),
    "--prompt",
    prompt_text,
  }, {
    stdin = opts.content or buffer_content(bufnr),
  }, callback)
end

function M.prefetch(path, callback)
  callback = callback or function() end
  local target = path

  if not target or target == "" then
    target = vim.fn.getcwd()
  end

  run_command({
    config.command,
    "prefetch",
    "--path",
    target,
  }, nil, callback)
end

function M.codex_sessions(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local args = {
    config.command,
    "codex-sessions",
    "--workspace",
    opts.workspace or workspace(),
    "--limit",
    tostring(opts.limit or config.session_limit),
  }
  if opts.all ~= false then
    table.insert(args, "--all")
  end
  run_command(args, nil, callback)
end

local function thread_list_args(opts)
  local args = {
    config.command,
    "thread",
    "list",
    "--workspace",
    opts.workspace or workspace(),
  }
  if opts.file then
    table.insert(args, "--file")
    table.insert(args, opts.file)
  end
  return args
end

function M.thread_list(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  run_command(thread_list_args(opts), {
    stdin = opts.content or "",
  }, callback)
end

function M.thread_create(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local args = {
    config.command,
    "thread",
    "create",
    "--workspace",
    opts.workspace or workspace(),
    "--file",
    opts.file,
    "--offset",
    tostring(opts.offset),
    "--prompt",
    opts.prompt,
  }
  if opts.selection_start then
    table.insert(args, "--selection-start")
    table.insert(args, tostring(opts.selection_start))
  end
  if opts.selection_end then
    table.insert(args, "--selection-end")
    table.insert(args, tostring(opts.selection_end))
  end
  run_command(args, { stdin = opts.content or "" }, callback)
end

function M.thread_link(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local args = {
    config.command,
    "thread",
    "link",
    "--thread-id",
    opts.thread_id,
    "--session-id",
    opts.session_id,
  }
  if opts.session_path then
    table.insert(args, "--session-path")
    table.insert(args, opts.session_path)
  end
  run_command(args, nil, callback)
end

function M.thread_resolve(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  run_command({
    config.command,
    "thread",
    "resolve",
    "--thread-id",
    opts.thread_id,
    "--workspace",
    opts.workspace or workspace(),
    "--started-after",
    tostring(opts.started_after),
  }, nil, callback)
end

function M.thread_attach(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  run_command({
    config.command,
    "thread",
    "attach",
    "--workspace",
    opts.workspace or workspace(),
    "--file",
    opts.file,
    "--offset",
    tostring(opts.offset),
    "--session-id",
    opts.session_id,
  }, { stdin = opts.content or "" }, callback)
end

function M.suggestions_to_complete_items(response)
  local suggestions = (((response or {}).result or {}).suggestions) or {}
  local items = {}

  for _, suggestion in ipairs(suggestions) do
    items[#items + 1] = {
      word = suggestion.insert_text,
      abbr = suggestion.label,
      menu = suggestion.detail or "harnessd",
      info = suggestion.documentation,
    }
  end

  return items
end

function M.preview_complete(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  if vim.bo[bufnr].modified then
    local err = "save the buffer before previewing cached harnessd completions"
    vim.notify(err, vim.log.levels.WARN)
    callback(nil, err)
    return
  end
  local cursor, err = position(bufnr)
  if not cursor then
    callback(nil, err)
    return
  end
  local changedtick = vim.api.nvim_buf_get_changedtick(bufnr)
  M.complete({ bufnr = bufnr, offset = cursor.offset, prefix = opts.prefix }, function(response, request_err)
    if request_err then
      vim.notify(request_err, vim.log.levels.ERROR)
      callback(nil, request_err)
      return
    end
    if not vim.api.nvim_buf_is_valid(bufnr) or vim.api.nvim_buf_get_changedtick(bufnr) ~= changedtick then
      callback(nil, "buffer changed while fetching completion")
      return
    end
    local suggestions = (((response or {}).result or {}).suggestions) or {}
    if #suggestions == 0 then
      clear_preview()
      vim.notify("no cached harnessd completion available", vim.log.levels.INFO)
      callback(response, nil)
      return
    end
    render_preview(bufnr, cursor, suggestions[1].insert_text, "complete")
    callback(response, nil)
  end)
end

function M.accept()
  if not preview then
    return
  end
  local active = preview
  if not vim.api.nvim_buf_is_valid(active.bufnr)
      or vim.api.nvim_get_current_buf() ~= active.bufnr
      or vim.api.nvim_buf_get_changedtick(active.bufnr) ~= active.changedtick then
    clear_preview()
    vim.notify("discarded harnessd suggestion because the buffer changed", vim.log.levels.WARN)
    return
  end
  local cursor = vim.api.nvim_win_get_cursor(0)
  if cursor[1] - 1 ~= active.row or cursor[2] ~= active.col then
    clear_preview()
    vim.notify("discarded harnessd suggestion because the cursor moved", vim.log.levels.WARN)
    return
  end

  local lines = vim.split(active.insert_text, "\n", { plain = true })
  clear_preview()
  vim.api.nvim_buf_set_text(active.bufnr, active.row, active.col, active.row, active.col, lines)
  local end_row = active.row + #lines - 1
  local end_col = #lines == 1 and active.col + #lines[1] or #lines[#lines]
  vim.api.nvim_win_set_cursor(0, { end_row + 1, end_col })
end

function M.inline_ask(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  M.dismiss()
  local source_bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  local file, file_err = current_file(source_bufnr)
  local cursor, cursor_err = position(source_bufnr)
  if not file or not cursor then
    vim.notify(file_err or cursor_err, vim.log.levels.ERROR)
    return
  end
  local content = buffer_content(source_bufnr)
  local prompt_bufnr = vim.api.nvim_create_buf(false, true)
  vim.bo[prompt_bufnr].buftype = "prompt"
  vim.fn.prompt_setprompt(prompt_bufnr, "> ")
  local width = math.max(30, math.min(80, vim.o.columns - 6))
  local winid = vim.api.nvim_open_win(prompt_bufnr, true, {
    relative = "editor",
    row = math.max(1, math.floor(vim.o.lines / 3)),
    col = math.max(0, math.floor((vim.o.columns - width) / 2)),
    width = width,
    height = 1,
    style = "minimal",
    border = "rounded",
    title = " Harnessd Ask ",
    title_pos = "center",
  })
  prompt = { bufnr = prompt_bufnr, winid = winid }

  local function submit()
    local value = vim.api.nvim_buf_get_lines(prompt_bufnr, 0, 1, false)[1] or ""
    value = value:gsub("^> ", "")
    if value:match("^%s*$") then
      vim.notify("inline prompt must not be empty", vim.log.levels.WARN)
      return
    end
    close_prompt()
    M.inline({
      bufnr = source_bufnr,
      file = file,
      offset = cursor.offset,
      content = content,
      prompt = value,
    }, function(response, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
        callback(nil, err)
        return
      end
      if not vim.api.nvim_buf_is_valid(source_bufnr)
          or buffer_content(source_bufnr) ~= content then
        local stale = "discarded harnessd suggestion because the buffer changed"
        vim.notify(stale, vim.log.levels.WARN)
        callback(nil, stale)
        return
      end
      local suggestion = ((response or {}).result or {}).suggestion
      if suggestion then
        render_preview(source_bufnr, cursor, suggestion.insert_text, "inline")
      end
      callback(response, nil)
    end)
  end

  vim.keymap.set("i", "<CR>", submit, { buffer = prompt_bufnr, nowait = true })
  vim.keymap.set({ "i", "n" }, "<Esc>", M.dismiss, { buffer = prompt_bufnr, nowait = true })
  vim.cmd("startinsert")
end

local function ensure_sidebar()
  if sidebar and sidebar.winid and vim.api.nvim_win_is_valid(sidebar.winid) then
    return sidebar.winid, sidebar.bufnr
  end
  vim.cmd("botright vertical new")
  local winid = vim.api.nvim_get_current_win()
  local bufnr = vim.api.nvim_get_current_buf()
  vim.api.nvim_win_set_width(winid, config.sidebar_width)
  vim.bo[bufnr].buftype = "nofile"
  vim.bo[bufnr].bufhidden = "hide"
  vim.bo[bufnr].swapfile = false
  vim.bo[bufnr].filetype = "harnessd-threads"
  sidebar = {
    winid = winid,
    bufnr = bufnr,
    mode = "list",
    items = {},
    terminal_buffers = {},
    attach = nil,
  }
  vim.keymap.set("n", "<CR>", function() M.sidebar_open_selected() end, { buffer = bufnr, nowait = true })
  vim.keymap.set("n", "q", function() M.sidebar_toggle() end, { buffer = bufnr, nowait = true })
  vim.keymap.set("n", "r", function() M.sidebar_refresh() end, { buffer = bufnr, nowait = true })
  return winid, bufnr
end

local function render_sidebar(items, title)
  local _, bufnr = ensure_sidebar()
  sidebar.items = items or {}
  local lines = { title or "harnessd threads", "" }
  if #sidebar.items == 0 then
    lines[#lines + 1] = "No Codex sessions or anchored threads yet."
  end
  for index, item in ipairs(sidebar.items) do
    if item.kind == "thread" then
      local marker = item.thread.status == "stale" and "!" or "H"
      local session = item.thread.codex_session_id and (" -> " .. item.thread.codex_session_id) or ""
      lines[#lines + 1] = string.format("%2d. [%s] %s:%s %s%s", index, marker, vim.fn.fnamemodify(item.thread.file, ":t"), item.thread.current_line, item.thread.prompt_preview or "", session)
    elseif item.kind == "session" then
      local preview = item.session.preview or item.session.cwd or ""
      local star = item.session.project_match and "*" or " "
      lines[#lines + 1] = string.format("%2d. [%s] %s  %s", index, star, item.session.id, preview)
    end
  end
  vim.bo[bufnr].modifiable = true
  vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)
  vim.bo[bufnr].modifiable = false
end

local function launch_codex(launch)
  local argv = vim.deepcopy(launch.argv or {})
  if argv[1] == "codex" then
    argv[1] = config.codex_command
  end
  local _, bufnr = ensure_sidebar()
  local term_bufnr = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_win_set_buf(sidebar.winid, term_bufnr)
  vim.api.nvim_set_current_win(sidebar.winid)
  vim.bo[term_bufnr].bufhidden = "hide"
  sidebar.terminal_buffers[table.concat(argv, "\0")] = term_bufnr
  if config.terminal_launcher then
    config.terminal_launcher(argv, { cwd = launch.cwd }, term_bufnr)
  else
    vim.fn.termopen(argv, { cwd = launch.cwd })
  end
  vim.cmd("startinsert")
  return term_bufnr, bufnr
end

local function launch_session(session, prompt_text)
  local argv = {
    config.codex_command,
    "--no-alt-screen",
    "resume",
    session.id,
  }
  if prompt_text and prompt_text ~= "" then
    argv[#argv + 1] = prompt_text
  end
  local launch = { argv = argv, cwd = session.cwd or workspace(), started_after_unix = os.time() }
  return launch_codex(launch)
end

local function mark_threads(bufnr, threads)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  sign_define()
  vim.api.nvim_buf_clear_namespace(bufnr, thread_namespace, 0, -1)
  pcall(vim.fn.sign_unplace, thread_sign_group, { buffer = bufnr })
  current_threads[bufnr] = threads or {}
  for _, thread in ipairs(threads or {}) do
    local row = math.max(0, (thread.current_line or 1) - 1)
    local hl = thread.status == "stale" and "DiagnosticWarn" or "DiagnosticInfo"
    vim.api.nvim_buf_set_extmark(bufnr, thread_namespace, row, 0, {
      virt_text = { { " " .. (thread.codex_session_id and "Codex thread" or "Codex ask"), hl } },
      virt_text_pos = "eol",
    })
    thread_sign_id = thread_sign_id + 1
    pcall(vim.fn.sign_place, thread_sign_id, thread_sign_group, thread.status == "stale" and "HarnessdThreadStale" or "HarnessdThread", bufnr, {
      lnum = row + 1,
      priority = 10,
    })
  end
end

function M.refresh_thread_marks(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  local file = select(1, current_file(bufnr))
  if not file then
    return
  end
  M.thread_list({
    workspace = workspace(),
    file = file,
    content = buffer_content(bufnr),
  }, function(response, err)
    if err then
      return
    end
    mark_threads(bufnr, (((response or {}).result or {}).threads) or {})
  end)
end

function M.sidebar_refresh(opts)
  opts = opts or {}
  local items = {}
  local source_bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  local file = select(1, current_file(source_bufnr))
  local content = file and buffer_content(source_bufnr) or ""
  M.thread_list({
    workspace = workspace(),
    file = file,
    content = content,
  }, function(thread_response)
    for _, thread in ipairs((((thread_response or {}).result or {}).threads) or {}) do
      items[#items + 1] = { kind = "thread", thread = thread }
    end
    M.codex_sessions({ workspace = workspace(), all = true }, function(session_response, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
      else
        for _, session in ipairs((((session_response or {}).result or {}).sessions) or {}) do
          items[#items + 1] = { kind = "session", session = session }
        end
      end
      render_sidebar(items, "harnessd Codex threads  <CR> open  r refresh  q close")
    end)
  end)
end

function M.sidebar_toggle()
  if sidebar and sidebar.winid and vim.api.nvim_win_is_valid(sidebar.winid) then
    vim.api.nvim_win_close(sidebar.winid, true)
    sidebar.winid = nil
    return
  end
  ensure_sidebar()
  M.sidebar_refresh()
end

function M.sidebar_open_selected()
  if not sidebar or not sidebar.bufnr or not vim.api.nvim_buf_is_valid(sidebar.bufnr) then
    return
  end
  local line = vim.api.nvim_win_get_cursor(sidebar.winid)[1]
  local item = sidebar.items[line - 2]
  if not item then
    return
  end
  if sidebar.attach and item.kind == "session" then
    local attach = sidebar.attach
    M.thread_attach({
      workspace = workspace(),
      file = attach.file,
      offset = attach.offset,
      content = attach.content,
      session_id = item.session.id,
    }, function(response, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
        return
      end
      mark_threads(attach.bufnr, { ((response or {}).result or {}).thread })
      sidebar.attach = nil
      M.sidebar_refresh({ bufnr = attach.bufnr })
    end)
    return
  end
  if item.kind == "thread" then
    local thread = item.thread
    if thread.codex_session_id then
      launch_session({ id = thread.codex_session_id, cwd = thread.workspace }, nil)
    else
      vim.notify("thread has no linked Codex session yet", vim.log.levels.WARN)
    end
  elseif item.kind == "session" then
    launch_session(item.session, nil)
  end
end

function M.thread_ask(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  M.dismiss()
  local source_bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  local file, file_err = current_file(source_bufnr)
  local cursor, cursor_err = position(source_bufnr)
  if not file or not cursor then
    vim.notify(file_err or cursor_err, vim.log.levels.ERROR)
    return
  end
  local content = buffer_content(source_bufnr)
  local prompt_bufnr = vim.api.nvim_create_buf(false, true)
  vim.bo[prompt_bufnr].buftype = "prompt"
  vim.fn.prompt_setprompt(prompt_bufnr, "> ")
  local width = math.max(30, math.min(80, vim.o.columns - 6))
  local winid = vim.api.nvim_open_win(prompt_bufnr, true, {
    relative = "editor",
    row = math.max(1, math.floor(vim.o.lines / 3)),
    col = math.max(0, math.floor((vim.o.columns - width) / 2)),
    width = width,
    height = 1,
    style = "minimal",
    border = "rounded",
    title = " Harnessd Thread ",
    title_pos = "center",
  })
  prompt = { bufnr = prompt_bufnr, winid = winid }

  local function submit()
    local value = vim.api.nvim_buf_get_lines(prompt_bufnr, 0, 1, false)[1] or ""
    value = value:gsub("^> ", "")
    if value:match("^%s*$") then
      vim.notify("thread prompt must not be empty", vim.log.levels.WARN)
      return
    end
    close_prompt()
    M.thread_create({
      workspace = workspace(),
      file = file,
      offset = cursor.offset,
      content = content,
      prompt = value,
    }, function(response, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
        callback(nil, err)
        return
      end
      local result = (response or {}).result or {}
      if result.thread then
        mark_threads(source_bufnr, { result.thread })
      end
      ensure_sidebar()
      M.sidebar_refresh({ bufnr = source_bufnr })
      if result.launch then
        launch_codex(result.launch)
        vim.defer_fn(function()
          M.thread_resolve({
            thread_id = result.thread.thread_id,
            workspace = workspace(),
            started_after = result.launch.started_after_unix or os.time(),
          }, function(resolve_response)
            local resolved_thread = (((resolve_response or {}).result or {}).thread)
            if resolved_thread then
              mark_threads(source_bufnr, { resolved_thread })
            end
          end)
        end, 1500)
      end
      callback(response, nil)
    end)
  end

  vim.keymap.set("i", "<CR>", submit, { buffer = prompt_bufnr, nowait = true })
  vim.keymap.set({ "i", "n" }, "<Esc>", M.dismiss, { buffer = prompt_bufnr, nowait = true })
  vim.cmd("startinsert")
end

function M.thread_open_current()
  local bufnr = vim.api.nvim_get_current_buf()
  local cursor = vim.api.nvim_win_get_cursor(0)
  for _, thread in ipairs(current_threads[bufnr] or {}) do
    if thread.current_line == cursor[1] and thread.codex_session_id then
      launch_session({ id = thread.codex_session_id, cwd = thread.workspace }, nil)
      return
    end
  end
  ensure_sidebar()
  M.sidebar_refresh({ bufnr = bufnr })
end

function M.thread_attach_current()
  local bufnr = vim.api.nvim_get_current_buf()
  local file, err = current_file(bufnr)
  local cursor, cursor_err = position(bufnr)
  if not file or not cursor then
    vim.notify(err or cursor_err, vim.log.levels.ERROR)
    return
  end
  ensure_sidebar()
  sidebar.attach = {
    bufnr = bufnr,
    file = file,
    offset = cursor.offset,
    content = buffer_content(bufnr),
  }
  M.sidebar_refresh({ bufnr = bufnr })
end

function M.setup(opts)
  merge_config(opts)

  vim.api.nvim_create_user_command("HarnessdPrefetch", function(command)
    local target = command.args ~= "" and command.args or vim.fn.getcwd()
    M.prefetch(target, function(_, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
        return
      end

      vim.notify("harnessd prefetch queued for " .. target, vim.log.levels.INFO)
    end)
  end, {
    nargs = "?",
    complete = "dir",
  })

  vim.api.nvim_create_user_command("HarnessdCompleteDebug", function()
    M.complete({}, function(response, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
        return
      end

      local suggestions = (((response or {}).result or {}).suggestions) or {}
      vim.notify(vim.inspect(suggestions), vim.log.levels.INFO)
    end)
  end, {})
  vim.api.nvim_create_user_command("HarnessdAsk", function() M.thread_ask() end, {})
  vim.api.nvim_create_user_command("HarnessdInline", function() M.inline_ask() end, {})
  vim.api.nvim_create_user_command("HarnessdThreads", function() M.sidebar_toggle() end, {})
  vim.api.nvim_create_user_command("HarnessdThreadOpen", function() M.thread_open_current() end, {})
  vim.api.nvim_create_user_command("HarnessdThreadAttach", function() M.thread_attach_current() end, {})
  vim.api.nvim_create_user_command("HarnessdComplete", function() M.preview_complete() end, {})
  vim.api.nvim_create_user_command("HarnessdAccept", function() M.accept() end, {})
  vim.api.nvim_create_user_command("HarnessdDismiss", function() M.dismiss() end, {})

  sign_define()
  vim.api.nvim_create_autocmd({ "BufEnter", "BufReadPost" }, {
    group = vim.api.nvim_create_augroup("harnessd_threads", { clear = true }),
    callback = function(event)
      M.refresh_thread_marks(event.buf)
    end,
  })

  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdAsk)", function() M.thread_ask() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdInline)", function() M.inline_ask() end)
  vim.keymap.set("n", "<Plug>(HarnessdThreads)", function() M.sidebar_toggle() end)
  vim.keymap.set("n", "<Plug>(HarnessdThreadOpen)", function() M.thread_open_current() end)
  vim.keymap.set("n", "<Plug>(HarnessdThreadAttach)", function() M.thread_attach_current() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdComplete)", function() M.preview_complete() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdAccept)", function() M.accept() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdDismiss)", function() M.dismiss() end)
end

return M
