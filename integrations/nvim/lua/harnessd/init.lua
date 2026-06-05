local M = {}

local config = {
  command = "harnessd",
  codex_command = "codex",
  sidebar_width = 72,
  session_limit = 50,
  thread_sign_text = "H",
  auto_inline = false,
  auto_inline_delay_ms = 250,
  fast_inline = true,
  fast_inline_background_refresh = true,
  inline_refresh_indicator_ms = 4000,
  prepare_context_on_buf_enter = true,
  inline_completion_prompt = "Complete the code at the cursor with the smallest useful insertion.",
  model_presets = {
    { label = "default", model = nil, reasoning_effort = nil },
    { label = "gpt-5.4-mini / low", model = "gpt-5.4-mini", reasoning_effort = "low" },
    { label = "gpt-5.4-mini / medium", model = "gpt-5.4-mini", reasoning_effort = "medium" },
    { label = "gpt-5.5 / medium", model = "gpt-5.5", reasoning_effort = "medium" },
    { label = "gpt-5.5 / high", model = "gpt-5.5", reasoning_effort = "high" },
  },
  model_roles = {
    ask = { model = nil, reasoning_effort = nil },
    line = { model = "gpt-5.4-mini", reasoning_effort = "low" },
    scratch = { model = nil, reasoning_effort = nil },
  },
}

local namespace = vim.api.nvim_create_namespace("harnessd_preview")
local thread_namespace = vim.api.nvim_create_namespace("harnessd_threads")
local preview = nil
local prompt = nil
local sidebar = nil
local thread_sign_group = "harnessd_threads"
local thread_sign_id = 1000
local current_threads = {}
local auto_inline_timer = nil
local auto_inline_running = false
local auto_inline_pending = false
local last_auto_inline_key = nil
local inline_refresh_timer = nil
local inline_status = {
  state = "idle",
  source = nil,
  message = nil,
  refresh = "idle",
  last_error = nil,
}
local context_status = {
  state = "idle",
  file = nil,
  message = nil,
  last_attempt_at = nil,
  last_ready_at = nil,
}
local buffer_models = {}
local settings_window = nil
local model_roles = { "ask", "line", "scratch" }
local render_settings_window

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

local function mark_offset(bufnr, mark)
  local pos = vim.api.nvim_buf_get_mark(bufnr, mark)
  if not pos or pos[1] <= 0 then
    return nil
  end
  local line_offset = vim.api.nvim_buf_get_offset(bufnr, pos[1] - 1)
  if line_offset < 0 then
    return nil
  end
  return line_offset + pos[2]
end

local function visual_selection(bufnr)
  local start_offset = mark_offset(bufnr, "<")
  local end_offset = mark_offset(bufnr, ">")
  if not start_offset or not end_offset then
    return nil, nil
  end
  if end_offset < start_offset then
    start_offset, end_offset = end_offset, start_offset
  end
  return start_offset, end_offset
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

local function insert_mode_active()
  local mode = vim.api.nvim_get_mode().mode
  return mode:sub(1, 1) == "i"
end

local function normalize_model(model)
  if model == nil then
    return nil
  end
  if type(model) ~= "string" then
    return nil
  end
  model = model:gsub("^%s+", ""):gsub("%s+$", "")
  if model == "" or model == "default" then
    return nil
  end
  return model
end

local function normalize_reasoning_effort(effort)
  if effort == nil then
    return nil
  end
  if type(effort) ~= "string" then
    return nil
  end
  effort = effort:gsub("^%s+", ""):gsub("%s+$", "")
  if effort == "" or effort == "default" then
    return nil
  end
  return effort
end

local function normalize_profile(value, effort)
  if type(value) == "table" then
    return {
      model = normalize_model(value.model or value.value),
      reasoning_effort = normalize_reasoning_effort(value.reasoning_effort or value.effort),
    }
  end
  return {
    model = normalize_model(value),
    reasoning_effort = normalize_reasoning_effort(effort),
  }
end

local function default_models()
  return {
    ask = normalize_profile(config.model_roles.ask),
    line = normalize_profile(config.model_roles.line),
    scratch = normalize_profile(config.model_roles.scratch),
  }
end

local function model_label(profile)
  profile = normalize_profile(profile)
  if not profile.model and not profile.reasoning_effort then
    return "default"
  end
  return (profile.model or "default") .. " / " .. (profile.reasoning_effort or "default")
end

local function add_model_args(args, model, effort)
  local profile = normalize_profile(model, effort)
  if profile.model then
    table.insert(args, "--model")
    table.insert(args, profile.model)
  end
  if profile.reasoning_effort then
    table.insert(args, "--reasoning-effort")
    table.insert(args, profile.reasoning_effort)
  end
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

local function stop_auto_inline_timer()
  if auto_inline_timer then
    pcall(function()
      auto_inline_timer:stop()
      auto_inline_timer:close()
    end)
    auto_inline_timer = nil
  end
end

local function stop_inline_refresh_timer()
  if inline_refresh_timer then
    pcall(function()
      inline_refresh_timer:stop()
      inline_refresh_timer:close()
    end)
    inline_refresh_timer = nil
  end
end

local function set_inline_status(next_status)
  inline_status = vim.tbl_extend("force", inline_status, next_status or {})
  vim.cmd("redrawstatus")
  if settings_window then
    render_settings_window()
  end
end

local function schedule_inline_refresh_reset()
  stop_inline_refresh_timer()
  inline_refresh_timer = vim.defer_fn(function()
    inline_refresh_timer = nil
    if inline_status.refresh == "queued" or inline_status.refresh == "running" then
      set_inline_status({ refresh = "idle" })
    end
  end, config.inline_refresh_indicator_ms)
end

local function insert_preview_text(text)
  if not preview then
    return false
  end
  local active = preview
  if not vim.api.nvim_buf_is_valid(active.bufnr)
      or vim.api.nvim_get_current_buf() ~= active.bufnr
      or vim.api.nvim_buf_get_changedtick(active.bufnr) ~= active.changedtick then
    clear_preview()
    vim.notify("discarded harnessd suggestion because the buffer changed", vim.log.levels.WARN)
    return true
  end
  local cursor = vim.api.nvim_win_get_cursor(0)
  if cursor[1] - 1 ~= active.row or cursor[2] ~= active.col then
    clear_preview()
    vim.notify("discarded harnessd suggestion because the cursor moved", vim.log.levels.WARN)
    return true
  end

  local lines = vim.split(text, "\n", { plain = true })
  clear_preview()
  vim.api.nvim_buf_set_text(active.bufnr, active.row, active.col, active.row, active.col, lines)
  local end_row = active.row + #lines - 1
  local end_col = #lines == 1 and active.col + #lines[1] or #lines[#lines]
  vim.api.nvim_win_set_cursor(0, { end_row + 1, end_col })
  return true
end

function M.dismiss()
  clear_preview()
  close_prompt()
end

function M.get_models(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  if not buffer_models[bufnr] then
    buffer_models[bufnr] = default_models()
  end
  return vim.deepcopy(buffer_models[bufnr])
end

function M.set_model(role, model, bufnr, reasoning_effort)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  if role ~= "ask" and role ~= "line" and role ~= "scratch" then
    return nil, "unknown model role: " .. tostring(role)
  end
  if not buffer_models[bufnr] then
    buffer_models[bufnr] = default_models()
  end
  buffer_models[bufnr][role] = normalize_profile(model, reasoning_effort)
  return buffer_models[bufnr][role].model
end

function M.model_for(role, bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  if not buffer_models[bufnr] then
    buffer_models[bufnr] = default_models()
  end
  return (buffer_models[bufnr][role] or {}).model
end

function M.reasoning_effort_for(role, bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  if not buffer_models[bufnr] then
    buffer_models[bufnr] = default_models()
  end
  return (buffer_models[bufnr][role] or {}).reasoning_effort
end

function M.is_auto_inline_enabled()
  return config.auto_inline == true
end

function M.set_auto_inline(enabled)
  config.auto_inline = enabled == true
  if config.auto_inline then
    M.schedule_auto_inline()
  else
    stop_auto_inline_timer()
    auto_inline_pending = false
    last_auto_inline_key = nil
    clear_preview()
  end
  vim.cmd("redrawstatus")
  return config.auto_inline
end

function M.toggle_auto_inline()
  return M.set_auto_inline(not config.auto_inline)
end

function M.statusline()
  local context = context_status.state or "idle"
  local inline = "idle"
  if inline_status.state == "failed" then
    inline = "failed"
  elseif inline_status.state == "waiting" then
    inline = "waiting"
  elseif inline_status.refresh == "queued" or inline_status.refresh == "running" then
    inline = "refresh"
  end
  if config.auto_inline then
    return "autocomplete: [on] context: [" .. context .. "] inline: [" .. inline .. "]"
  end
  return "autocomplete: [off] context: [" .. context .. "] inline: [" .. inline .. "]"
end

function M.context_status()
  return vim.deepcopy(context_status)
end

function M.inline_status()
  return vim.deepcopy(inline_status)
end

function M.prepare_inline(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  if config.prepare_context_on_buf_enter == false and not opts.force then
    callback(nil, "context preparation disabled")
    return
  end

  local bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  if vim.bo[bufnr].buftype ~= "" then
    callback(nil, "context preparation requires a normal file buffer")
    return
  end

  local file, file_err = current_file(bufnr)
  if not file then
    callback(nil, file_err)
    return
  end
  if context_status.file == file and context_status.state == "loading" then
    callback(nil, "context preparation already running")
    return
  end

  context_status = {
    state = "loading",
    file = file,
    message = "preparing",
    last_attempt_at = os.time(),
    last_ready_at = context_status.last_ready_at,
  }
  vim.cmd("redrawstatus")
  if settings_window then
    render_settings_window()
  end

  local args = {
    config.command,
    "bridge",
    "--method",
    "inline.prepare",
    "--file",
    file,
  }
  add_model_args(
    args,
    opts.model or M.model_for("line", bufnr),
    opts.reasoning_effort or M.reasoning_effort_for("line", bufnr)
  )

  run_command(args, nil, function(response, err)
    if err then
      context_status = {
        state = "failed",
        file = file,
        message = err,
        last_attempt_at = context_status.last_attempt_at,
        last_ready_at = context_status.last_ready_at,
      }
      vim.cmd("redrawstatus")
      if settings_window then
        render_settings_window()
      end
      callback(nil, err)
      return
    end

    context_status = {
      state = "ready",
      file = file,
      message = nil,
      last_attempt_at = context_status.last_attempt_at,
      last_ready_at = os.time(),
    }
    vim.cmd("redrawstatus")
    if settings_window then
      render_settings_window()
    end
    callback(response, nil)
  end)
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

  local args = {
    config.command,
    "inline",
    "--file",
    file,
    "--offset",
    tostring(offset),
    "--prompt",
    prompt_text,
  }
  add_model_args(args, opts.model, opts.reasoning_effort)

  run_command(args, {
    stdin = opts.content or buffer_content(bufnr),
  }, callback)
end

function M.inline_fast(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  local file = opts.file or select(1, current_file(bufnr))
  if not file then
    callback(nil, "file path is required")
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

  local args = {
    config.command,
    "bridge",
    "--method",
    "inline.fast",
    "--file",
    file,
    "--cursor",
    tostring(offset),
  }
  if opts.prompt and opts.prompt ~= "" then
    table.insert(args, "--text")
    table.insert(args, opts.prompt)
  end
  add_model_args(args, opts.model, opts.reasoning_effort)
  if opts.allow_background_refresh == false then
    table.insert(args, "--no-background-refresh")
  end

  run_command(args, {
    stdin = opts.content or buffer_content(bufnr),
  }, callback)
end

function M.inline_complete(opts, callback)
  opts = opts or {}
  callback = callback or function() end
  local source_bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  local file, file_err = current_file(source_bufnr)
  local cursor, cursor_err = position(source_bufnr)
  if opts.require_insert_mode and not insert_mode_active() then
    callback(nil, "insert mode is no longer active")
    return
  end
  if vim.bo[source_bufnr].buftype ~= "" then
    callback(nil, "harnessd inline completion requires a normal file buffer")
    return
  end
  if not file or not cursor then
    local err = file_err or cursor_err
    if not opts.silent then
      vim.notify(err, vim.log.levels.ERROR)
    end
    callback(nil, err)
    return
  end

  local content = buffer_content(source_bufnr)
  local changedtick = vim.api.nvim_buf_get_changedtick(source_bufnr)
  local request = config.fast_inline and M.inline_fast or M.inline
  set_inline_status({
    state = "waiting",
    source = nil,
    message = "requesting",
    last_error = nil,
  })
  request({
    bufnr = source_bufnr,
    file = file,
    offset = cursor.offset,
    content = content,
    prompt = opts.prompt or config.inline_completion_prompt,
    model = opts.model or M.model_for("line", source_bufnr),
    reasoning_effort = opts.reasoning_effort or M.reasoning_effort_for("line", source_bufnr),
    allow_background_refresh = config.fast_inline_background_refresh,
  }, function(response, err)
    if err then
      set_inline_status({
        state = "failed",
        message = err,
        last_error = err,
        refresh = "idle",
      })
      if not opts.silent then
        vim.notify(err, vim.log.levels.ERROR)
      end
      callback(nil, err)
      return
    end
    if opts.require_insert_mode and not insert_mode_active() then
      set_inline_status({
        state = "idle",
        message = "stale",
      })
      callback(nil, "insert mode is no longer active")
      return
    end
    if not vim.api.nvim_buf_is_valid(source_bufnr)
        or vim.api.nvim_get_current_buf() ~= source_bufnr
        or vim.api.nvim_buf_get_changedtick(source_bufnr) ~= changedtick
        or buffer_content(source_bufnr) ~= content then
      set_inline_status({
        state = "idle",
        message = "stale",
      })
      callback(nil, "buffer changed while fetching inline completion")
      return
    end
    local current_cursor = vim.api.nvim_win_get_cursor(0)
    if current_cursor[1] - 1 ~= cursor.row or current_cursor[2] ~= cursor.col then
      set_inline_status({
        state = "idle",
        message = "stale",
      })
      callback(nil, "cursor moved while fetching inline completion")
      return
    end

    local result = (response or {}).result or {}
    local suggestion = result.suggestion
    if suggestion then
      stop_inline_refresh_timer()
      set_inline_status({
        state = "idle",
        source = result.source or "generated",
        message = nil,
        refresh = "idle",
      })
      render_preview(source_bufnr, cursor, suggestion.insert_text, opts.source or "inline_complete")
    elseif result.refresh_queued == true then
      set_inline_status({
        state = "idle",
        source = result.source or "none",
        message = "refresh queued",
        refresh = "queued",
      })
      schedule_inline_refresh_reset()
    else
      set_inline_status({
        state = "idle",
        source = result.source or "none",
        message = "no suggestion",
      })
    end
    callback(response, nil)
  end)
end

function M.schedule_auto_inline()
  if not config.auto_inline or not insert_mode_active() then
    return
  end
  local bufnr = vim.api.nvim_get_current_buf()
  if vim.bo[bufnr].buftype ~= "" or vim.api.nvim_buf_get_name(bufnr) == "" then
    return
  end
  clear_preview()

  stop_auto_inline_timer()

  auto_inline_timer = vim.defer_fn(function()
    auto_inline_timer = nil
    if not config.auto_inline or not insert_mode_active() then
      return
    end
    local target_bufnr = vim.api.nvim_get_current_buf()
    local cursor = select(1, position(target_bufnr))
    if not cursor then
      return
    end
    local key = table.concat({
      tostring(target_bufnr),
      tostring(vim.api.nvim_buf_get_changedtick(target_bufnr)),
      tostring(cursor.row),
      tostring(cursor.col),
    }, ":")
    if key == last_auto_inline_key then
      return
    end
    if auto_inline_running then
      auto_inline_pending = true
      return
    end

    auto_inline_running = true
    last_auto_inline_key = key
    M.inline_complete({
      bufnr = target_bufnr,
      prompt = config.inline_completion_prompt,
      require_insert_mode = true,
      silent = true,
      source = "auto_inline",
    }, function()
      auto_inline_running = false
      if auto_inline_pending then
        auto_inline_pending = false
        M.schedule_auto_inline()
      end
    end)
  end, config.auto_inline_delay_ms)
end

function M.scratch(opts, callback)
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
    callback(nil, "scratch prompt must not be empty")
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

  local args = {
    config.command,
    "scratch",
    "--workspace",
    opts.workspace or workspace(),
    "--file",
    file,
    "--offset",
    tostring(offset),
    "--prompt",
    prompt_text,
  }
  if opts.selection_start ~= nil and opts.selection_end ~= nil then
    table.insert(args, "--selection-start")
    table.insert(args, tostring(opts.selection_start))
    table.insert(args, "--selection-end")
    table.insert(args, tostring(opts.selection_end))
  end
  add_model_args(args, opts.model, opts.reasoning_effort)

  run_command(args, {
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
  add_model_args(args, opts.model, opts.reasoning_effort)
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
    return false
  end
  return insert_preview_text(preview.insert_text)
end

function M.accept_line()
  if not preview then
    return false
  end
  local first_line = vim.split(preview.insert_text, "\n", { plain = true })[1] or ""
  return insert_preview_text(first_line)
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
      model = opts.model or M.model_for("line", source_bufnr),
      reasoning_effort = opts.reasoning_effort or M.reasoning_effort_for("line", source_bufnr),
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
    active_terminal = nil,
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
  local job_id = nil
  if config.terminal_launcher then
    job_id = config.terminal_launcher(argv, { cwd = launch.cwd }, term_bufnr)
  else
    job_id = vim.fn.termopen(argv, { cwd = launch.cwd })
  end
  sidebar.active_terminal = {
    job_id = job_id,
    argv = argv,
    cwd = launch.cwd,
    bufnr = term_bufnr,
  }
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

function M.send_model_to_active_thread(model)
  model = normalize_model(model or M.model_for("ask"))
  if not model then
    vim.notify("/model requires a concrete ask model", vim.log.levels.WARN)
    return false
  end
  if not sidebar or not sidebar.active_terminal or not sidebar.active_terminal.job_id then
    vim.notify("no active Codex terminal for /model", vim.log.levels.WARN)
    return false
  end
  vim.api.nvim_chan_send(sidebar.active_terminal.job_id, "/model " .. model .. "\n")
  vim.notify("sent /model " .. model, vim.log.levels.INFO)
  return true
end

local function close_settings_window()
  if settings_window and settings_window.winid and vim.api.nvim_win_is_valid(settings_window.winid) then
    vim.api.nvim_win_close(settings_window.winid, true)
  end
  if settings_window and settings_window.bufnr and vim.api.nvim_buf_is_valid(settings_window.bufnr) then
    vim.api.nvim_buf_delete(settings_window.bufnr, { force = true })
  end
  settings_window = nil
end

local function setting_at_cursor()
  if not settings_window or not settings_window.winid or not vim.api.nvim_win_is_valid(settings_window.winid) then
    return nil
  end
  local row = vim.api.nvim_win_get_cursor(settings_window.winid)[1]
  return settings_window.rows and settings_window.rows[row]
end

local function compact_status_text(value, limit)
  value = tostring(value or "none"):gsub("%s+", " ")
  limit = limit or 48
  if #value > limit then
    return value:sub(1, limit - 1) .. "..."
  end
  return value
end

function render_settings_window()
  if not settings_window or not vim.api.nvim_buf_is_valid(settings_window.bufnr) then
    return
  end
  local models = M.get_models(settings_window.source_bufnr)
  local lines = { "harnessd settings", "", "models" }
  local rows = {}
  for _, role in ipairs(model_roles) do
    rows[#lines + 1] = { kind = "model", role = role }
    lines[#lines + 1] = string.format("%-8s %s", role, model_label(models[role]))
  end
  lines[#lines + 1] = ""
  lines[#lines + 1] = "behavior"
  rows[#lines + 1] = { kind = "toggle", key = "auto_inline" }
  lines[#lines + 1] = string.format("%-16s %s", "auto inline", config.auto_inline and "on" or "off")
  rows[#lines + 1] = { kind = "toggle", key = "prepare_context_on_buf_enter" }
  lines[#lines + 1] = string.format(
    "%-16s %s",
    "prepare context",
    config.prepare_context_on_buf_enter and "on" or "off"
  )
  lines[#lines + 1] = ""
  lines[#lines + 1] = "status"
  rows[#lines + 1] = { kind = "status" }
  lines[#lines + 1] = string.format("%-16s %s", "context", context_status.state or "idle")
  rows[#lines + 1] = { kind = "status" }
  lines[#lines + 1] = string.format("%-16s %s", "inline", inline_status.state or "idle")
  rows[#lines + 1] = { kind = "status" }
  lines[#lines + 1] = string.format("%-16s %s", "refresh", inline_status.refresh or "idle")
  rows[#lines + 1] = { kind = "status" }
  lines[#lines + 1] = string.format("%-16s %s", "last error", compact_status_text(inline_status.last_error))
  lines[#lines + 1] = ""
  lines[#lines + 1] = "<CR> change/toggle  r reset  / send ask /model  q close"
  settings_window.rows = rows
  vim.bo[settings_window.bufnr].modifiable = true
  vim.api.nvim_buf_set_lines(settings_window.bufnr, 0, -1, false, lines)
  vim.bo[settings_window.bufnr].modifiable = false
end

local function choose_model_for_role(role)
  if not role then
    return
  end
  vim.ui.select(config.model_presets, {
    prompt = "Harnessd " .. role .. " model",
    format_item = function(item)
      return item.label or model_label(item)
    end,
  }, function(choice)
    if not choice then
      return
    end
    M.set_model(role, choice, settings_window and settings_window.source_bufnr or nil)
    render_settings_window()
  end)
end

local function change_setting(setting)
  if not setting then
    return
  end
  if setting.kind == "model" then
    choose_model_for_role(setting.role)
  elseif setting.kind == "toggle" then
    if setting.key == "auto_inline" then
      M.set_auto_inline(not config.auto_inline)
    else
      config[setting.key] = not config[setting.key]
      vim.cmd("redrawstatus")
    end
    render_settings_window()
  end
end

local function reset_setting(setting)
  if not setting then
    return
  end
  if setting.kind == "model" then
    M.set_model(setting.role, config.model_roles[setting.role], settings_window.source_bufnr)
  elseif setting.kind == "toggle" then
    if setting.key == "auto_inline" then
      M.set_auto_inline(false)
    elseif setting.key == "prepare_context_on_buf_enter" then
      config.prepare_context_on_buf_enter = true
      vim.cmd("redrawstatus")
    end
  end
  render_settings_window()
end

function M.open_settings(opts)
  opts = opts or {}
  local source_bufnr = opts.bufnr or vim.api.nvim_get_current_buf()
  close_settings_window()
  local bufnr = vim.api.nvim_create_buf(false, true)
  vim.bo[bufnr].buftype = "nofile"
  vim.bo[bufnr].bufhidden = "wipe"
  vim.bo[bufnr].swapfile = false
  vim.bo[bufnr].filetype = "harnessd-settings"
  local width = math.max(44, math.min(72, vim.o.columns - 6))
  local height = 11
  local winid = vim.api.nvim_open_win(bufnr, true, {
    relative = "editor",
    row = math.max(1, math.floor((vim.o.lines - height) / 3)),
    col = math.max(0, math.floor((vim.o.columns - width) / 2)),
    width = width,
    height = height,
    style = "minimal",
    border = "rounded",
    title = " Harnessd Settings ",
    title_pos = "center",
  })
  settings_window = { bufnr = bufnr, winid = winid, source_bufnr = source_bufnr, rows = {} }
  render_settings_window()
  vim.keymap.set("n", "q", close_settings_window, { buffer = bufnr, nowait = true })
  vim.keymap.set("n", "<CR>", function()
    change_setting(setting_at_cursor())
  end, { buffer = bufnr, nowait = true })
  vim.keymap.set("n", "r", function()
    reset_setting(setting_at_cursor())
  end, { buffer = bufnr, nowait = true })
  vim.keymap.set("n", "/", function()
    local setting = setting_at_cursor()
    if setting and setting.kind == "model" and setting.role == "ask" then
      M.send_model_to_active_thread(M.model_for("ask", source_bufnr))
    else
      vim.notify("/model applies to the ask role", vim.log.levels.WARN)
    end
  end, { buffer = bufnr, nowait = true })
end

function M.open_models(opts)
  return M.open_settings(opts)
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

function M.scratch_ask(opts, callback)
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
  local selection_start = opts.selection_start
  local selection_end = opts.selection_end
  if opts.use_visual_selection then
    selection_start, selection_end = visual_selection(source_bufnr)
  end

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
    title = " Harnessd Scratch ",
    title_pos = "center",
  })
  prompt = { bufnr = prompt_bufnr, winid = winid }

  local function submit()
    local value = vim.api.nvim_buf_get_lines(prompt_bufnr, 0, 1, false)[1] or ""
    value = value:gsub("^> ", "")
    if value:match("^%s*$") then
      vim.notify("scratch prompt must not be empty", vim.log.levels.WARN)
      return
    end
    close_prompt()
    M.scratch({
      bufnr = source_bufnr,
      workspace = workspace(),
      file = file,
      offset = cursor.offset,
      content = content,
      prompt = value,
      selection_start = selection_start,
      selection_end = selection_end,
      model = opts.model or M.model_for("scratch", source_bufnr),
      reasoning_effort = opts.reasoning_effort or M.reasoning_effort_for("scratch", source_bufnr),
    }, function(response, err)
      if err then
        vim.notify(err, vim.log.levels.ERROR)
        callback(nil, err)
        return
      end
      local result = (response or {}).result or {}
      if result.relative_path then
        vim.notify("harnessd scratch: " .. result.relative_path, vim.log.levels.INFO)
      end
      callback(response, nil)
    end)
  end

  vim.keymap.set("i", "<CR>", submit, { buffer = prompt_bufnr, nowait = true })
  vim.keymap.set({ "i", "n" }, "<Esc>", M.dismiss, { buffer = prompt_bufnr, nowait = true })
  vim.cmd("startinsert")
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
      model = opts.model or M.model_for("ask", source_bufnr),
      reasoning_effort = opts.reasoning_effort or M.reasoning_effort_for("ask", source_bufnr),
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
  vim.api.nvim_create_user_command("HarnessdScratch", function(command)
    M.scratch_ask({ use_visual_selection = command.range and command.range > 0 })
  end, { range = true })
  vim.api.nvim_create_user_command("HarnessdThreads", function() M.sidebar_toggle() end, {})
  vim.api.nvim_create_user_command("HarnessdThreadOpen", function() M.thread_open_current() end, {})
  vim.api.nvim_create_user_command("HarnessdThreadAttach", function() M.thread_attach_current() end, {})
  vim.api.nvim_create_user_command("HarnessdComplete", function() M.preview_complete() end, {})
  vim.api.nvim_create_user_command("HarnessdSettings", function() M.open_settings() end, {})
  vim.api.nvim_create_user_command("HarnessdModels", function() M.open_settings() end, {})
  vim.api.nvim_create_user_command("HarnessdInlineComplete", function()
    M.inline_complete({ silent = false })
  end, {})
  vim.api.nvim_create_user_command("HarnessdPrepareContext", function()
    M.prepare_inline({ force = true }, function(_, err)
      if err then
        vim.notify(err, vim.log.levels.WARN)
      end
    end)
  end, {})
  vim.api.nvim_create_user_command("HarnessdAccept", function() M.accept() end, {})
  vim.api.nvim_create_user_command("HarnessdDismiss", function() M.dismiss() end, {})

  sign_define()
  vim.api.nvim_create_autocmd({ "BufEnter", "BufReadPost" }, {
    group = vim.api.nvim_create_augroup("harnessd_threads", { clear = true }),
    callback = function(event)
      M.refresh_thread_marks(event.buf)
    end,
  })
  vim.api.nvim_create_autocmd({ "BufEnter", "BufReadPost" }, {
    group = vim.api.nvim_create_augroup("harnessd_context_prepare", { clear = true }),
    callback = function(event)
      M.prepare_inline({ bufnr = event.buf }, function() end)
    end,
  })

  vim.api.nvim_create_autocmd({ "InsertEnter", "TextChangedI", "CursorMovedI" }, {
    group = vim.api.nvim_create_augroup("harnessd_auto_inline", { clear = true }),
    callback = function()
      M.schedule_auto_inline()
    end,
  })
  vim.api.nvim_create_autocmd("InsertLeave", {
    group = vim.api.nvim_create_augroup("harnessd_auto_inline_cleanup", { clear = true }),
    callback = function()
      stop_auto_inline_timer()
      last_auto_inline_key = nil
      clear_preview()
    end,
  })

  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdAsk)", function() M.thread_ask() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdInline)", function() M.inline_ask() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdInlineComplete)", function() M.inline_complete() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdPrepareContext)", function()
    M.prepare_inline({ force = true }, function() end)
  end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdScratch)", function() M.scratch_ask() end)
  vim.keymap.set("v", "<Plug>(HarnessdScratch)", function() M.scratch_ask({ use_visual_selection = true }) end)
  vim.keymap.set("n", "<Plug>(HarnessdThreads)", function() M.sidebar_toggle() end)
  vim.keymap.set("n", "<Plug>(HarnessdThreadOpen)", function() M.thread_open_current() end)
  vim.keymap.set("n", "<Plug>(HarnessdThreadAttach)", function() M.thread_attach_current() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdComplete)", function() M.preview_complete() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdSettings)", function() M.open_settings() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdModels)", function() M.open_settings() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdAccept)", function() M.accept() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdAcceptLine)", function() M.accept_line() end)
  vim.keymap.set({ "n", "i" }, "<Plug>(HarnessdDismiss)", function() M.dismiss() end)
end

return M
