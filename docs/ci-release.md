# CI and releases

The `CI` workflow runs on pull requests into `main` and on pushes to `main`.
It checks Rust formatting and Clippy, runs the Rust tests, and builds the
server. The Python and C# suites run against that binary. The C# suite also
starts a Python worker and verifies the cross-language round trip.

`CI required` succeeds when all three jobs pass. Each successful run uploads
the Linux x64 server archive, Python wheel and source distribution, and C#
NuGet package as workflow artifacts.

## Set up publishing

Both SDKs use Trusted Publishing with short-lived credentials. Configure the
registries for `ChopstickLeg/DQ` and workflow file `release.yml`.

| Registry | Package | GitHub environment | Setup |
| --- | --- | --- | --- |
| PyPI | `dq-python` | `pypi` | Register a [pending publisher](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/) for the first release. |
| NuGet | `Dq.Client` | `nuget` | Create a [Trusted Publishing policy](https://learn.microsoft.com/en-us/nuget/nuget-org/trusted-publishing) scoped to `Dq.Client`. Allow new packages for the first release and new versions for later releases. Set `NUGET_USER` in the `nuget` environment to the NuGet.org profile name. |

The GitHub environments accept deployment from version tags matching `v*`.
The packages published to PyPI and NuGet are public.

## Release a version

1. Set the same version in `Cargo.toml`, `sdks/python/dq_python/pyproject.toml`,
   and `sdks/csharp/Dq.Client/Dq.Client.csproj`. Update `Cargo.lock` and merge the
   version changes into `main`.
2. Tag the versioned commit on `main` with `v` followed by the version, such as
   `v0.1.0`, and push the tag. The workflow checks the tag format, versions, and
   ancestry before running the full CI suite with a release build of the server.
3. Check the `Release` run. The workflow publishes the SDKs to their registries
   and attaches the tested Linux x64 server archive to a GitHub Release.
