param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [Parameter(Mandatory = $true)]
    [string]$Cursor,

    [string]$Harnessd = "harnessd"
)

& $Harnessd bridge --method complete --file $FilePath --cursor $Cursor
exit $LASTEXITCODE
