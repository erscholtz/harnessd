local M = {}

local config = {
  command = "harnessd",
}

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

local function current_offset(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  local cursor = vim.api.nvim_win_get_cursor(0)
  local line = cursor[1] - 1
  local col = cursor[2]
  local line_offset = vim.api.nvim_buf_get_offset(bufnr, line)

  if line_offset < 0 then
    return nil, "could not resolve buffer byte offset"
  end

  return line_offset + col, nil
end

local function run_command(args, callback)
  if vim.system == nil then
    callback(nil, "vim.system() is required (Neovim 0.10+)")
    return
  end

  vim.system(args, { text = true }, function(result)
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

function M.complete(opts, callback)
  opts = opts or {}
  callback = callback or function() end

  local file = opts.file
  if not file then
    file = select(1, current_file(opts.bufnr))
  end
  if not file then
    callback(nil, "file path is required")
    return
  end

  local offset = opts.offset
  if offset == nil then
    offset = select(1, current_offset(opts.bufnr))
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

  run_command(args, callback)
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
  }, callback)
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
end

return M
