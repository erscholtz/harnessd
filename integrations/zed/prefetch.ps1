param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [string]$Harnessd = "harnessd"
)

& $Harnessd bridge --method prefetch --file $Path
exit $LASTEXITCODE
