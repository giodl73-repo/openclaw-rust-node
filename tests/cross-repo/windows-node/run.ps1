[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$RustNodeRoot,

    [Parameter(Mandatory = $true)]
    [string]$WindowsNodeRoot,

    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$manifest = Get-Content -LiteralPath (Join-Path $scriptRoot "composition.json") -Raw |
    ConvertFrom-Json
$rustRoot = (Resolve-Path -LiteralPath $RustNodeRoot).Path
$windowsRoot = (Resolve-Path -LiteralPath $WindowsNodeRoot).Path

function Invoke-GitText {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $output = & git -C $RepositoryRoot @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed in ${RepositoryRoot}: $output"
    }
    return ($output | Out-String).Trim()
}

function Assert-GitSuccess {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [string]$FailureMessage
    )

    & git -C $RepositoryRoot @Arguments *> $null
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Executable,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [string]$WorkingDirectory
    )

    Push-Location $WorkingDirectory
    try {
        & $Executable @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Executable exited with code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

$sharedCommit = [string]$manifest.sharedRuntime.commit
$windowsCommit = [string]$manifest.windowsAdopter.commit
$probeCommit = [string]$manifest.processProbe.commit
$probeManifest = Join-Path $rustRoot ([string]$manifest.processProbe.manifest)
$windowsHead = Invoke-GitText -RepositoryRoot $windowsRoot -Arguments @("rev-parse", "HEAD")
if ($windowsHead -ne $windowsCommit) {
    throw "Windows adopter checkout is $windowsHead; expected exact pin $windowsCommit"
}

$dirtySharedSurface = Invoke-GitText -RepositoryRoot $rustRoot -Arguments @(
    "status", "--porcelain", "--untracked-files=all", "--",
    "crates", "test/fixtures", "tests/windows-sidecar-process"
)
if ($dirtySharedSurface) {
    throw "Shared crates or fixtures contain uncommitted changes: $dirtySharedSurface"
}
$dirtyWindowsSurface = Invoke-GitText -RepositoryRoot $windowsRoot -Arguments @(
    "status", "--porcelain", "--untracked-files=all", "--",
    "src/OpenClaw.Shared/RustSidecar",
    "tests/OpenClaw.Shared.Tests/RustSidecar"
)
if ($dirtyWindowsSurface) {
    throw "Windows sidecar implementation or tests contain uncommitted changes: $dirtyWindowsSurface"
}

Assert-GitSuccess -RepositoryRoot $rustRoot `
    -Arguments @("merge-base", "--is-ancestor", $sharedCommit, "HEAD") `
    -FailureMessage "Shared runtime pin $sharedCommit is not an ancestor of the candidate"
Assert-GitSuccess -RepositoryRoot $rustRoot `
    -Arguments @("diff", "--quiet", $sharedCommit, "HEAD", "--", "crates", "test/fixtures") `
    -FailureMessage "Shared crates or fixtures differ from the validated runtime pin $sharedCommit"
Assert-GitSuccess -RepositoryRoot $rustRoot `
    -Arguments @("merge-base", "--is-ancestor", $probeCommit, "HEAD") `
    -FailureMessage "Rust process-probe pin $probeCommit is not an ancestor of the candidate"
Assert-GitSuccess -RepositoryRoot $rustRoot `
    -Arguments @("diff", "--quiet", $probeCommit, "HEAD", "--", "tests/windows-sidecar-process") `
    -FailureMessage "Rust process probe differs from the validated pin $probeCommit"

$rustFixtureRoot = "test/fixtures"
$windowsFixtureRoot = "tests/OpenClaw.Shared.Tests/RustSidecar/Fixtures"
foreach ($fixture in $manifest.fixtures) {
    $name = [string]$fixture.name
    $expectedBlob = [string]$fixture.gitBlob
    $rustBlob = Invoke-GitText -RepositoryRoot $rustRoot `
        -Arguments @("rev-parse", "${sharedCommit}:${rustFixtureRoot}/${name}")
    $windowsBlob = Invoke-GitText -RepositoryRoot $windowsRoot `
        -Arguments @("rev-parse", "${windowsCommit}:${windowsFixtureRoot}/${name}")
    if ($rustBlob -ne $expectedBlob -or $windowsBlob -ne $expectedBlob) {
        throw "Fixture drift for ${name}: expected $expectedBlob, Rust $rustBlob, Windows $windowsBlob"
    }
    Write-Host "fixture $name blob=$expectedBlob"
}

if (-not $SkipTests) {
    Invoke-Checked -Executable "cargo" -WorkingDirectory $rustRoot -Arguments @(
        "test",
        "--manifest-path", (Join-Path $rustRoot "crates/Cargo.toml"),
        "--workspace",
        "--locked"
    )
    Invoke-Checked -Executable "cargo" -WorkingDirectory $rustRoot -Arguments @(
        "build", "--manifest-path", $probeManifest, "--locked"
    )
    $probePath = Join-Path (Split-Path -Parent $probeManifest) `
        "target/debug/openclaw-windows-sidecar-process-probe.exe"
    $previousProbe = $env:OPENCLAW_RUST_SIDECAR_PROBE
    try {
        $env:OPENCLAW_RUST_SIDECAR_PROBE = $probePath
        Invoke-Checked -Executable "dotnet" -WorkingDirectory $windowsRoot -Arguments @(
            "test",
            (Join-Path $windowsRoot "tests/OpenClaw.Shared.Tests/OpenClaw.Shared.Tests.csproj"),
            "--filter", "FullyQualifiedName~RustSidecar"
        )
    }
    finally {
        $env:OPENCLAW_RUST_SIDECAR_PROBE = $previousProbe
    }
}

Write-Host "cross-repo-conformance shared=$sharedCommit windows=$windowsCommit probe=$probeCommit fixtures=$($manifest.fixtures.Count) tests=$(-not $SkipTests)"
