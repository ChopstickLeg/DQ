using System.Diagnostics;

namespace Dq.Client.IntegrationTests;

internal sealed class RustServer(Process process, DirectoryInfo dataDirectory) : IAsyncDisposable
{
    public string Address { get; private set; } = "";

    public static async Task<RustServer> StartAsync(CancellationToken cancellationToken)
    {
        var binary = Environment.GetEnvironmentVariable("DQ_SERVER_BIN") ?? FindDefaultBinary();
        if (!File.Exists(binary))
        {
            throw new FileNotFoundException("Build the Rust server with cargo build --locked or set DQ_SERVER_BIN.", binary);
        }

        var directory = Directory.CreateTempSubdirectory("dq-csharp-");
        var startInfo = new ProcessStartInfo
        {
            FileName = Path.GetFullPath(binary),
            WorkingDirectory = directory.FullName,
            RedirectStandardOutput = true,
            UseShellExecute = false,
        };
        startInfo.Environment["DATABASE_URL"] = "sqlite://dq.db";
        startInfo.Environment["LISTEN_ADDR"] = "127.0.0.1:0";
        startInfo.Environment["OTEL_SDK_DISABLED"] = "true";

        RustServer? server = null;
        try
        {
            var child = Process.Start(startInfo) ?? throw new InvalidOperationException("Could not start the Rust server.");
            server = new RustServer(child, directory);
            var line = await child.StandardOutput.ReadLineAsync(cancellationToken);
            const string prefix = "Listening on ";
            if (line is null || !line.StartsWith(prefix, StringComparison.Ordinal))
            {
                throw new InvalidOperationException($"Unexpected Rust server startup output: {line ?? "<end of output>"}");
            }

            server.Address = $"http://{line[prefix.Length..]}";
            return server;
        }
        catch
        {
            if (server is null)
            {
                directory.Delete();
            }
            else
            {
                await server.DisposeAsync();
            }
            throw;
        }
    }

    public async ValueTask DisposeAsync()
    {
        using (process)
        {
            try
            {
                process.Kill();
            }
            catch (InvalidOperationException) when (process.HasExited)
            {
                // The child already stopped.
            }

            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            await process.WaitForExitAsync(timeout.Token);
        }

        // Windows can briefly retain SQLite file handles after the child exits.
        for (var attempt = 0; ; attempt++)
        {
            try
            {
                dataDirectory.Delete(recursive: true);
                return;
            }
            catch (IOException) when (OperatingSystem.IsWindows() && attempt < 10)
            {
                await Task.Delay(TimeSpan.FromMilliseconds(100));
            }
        }
    }

    private static string FindDefaultBinary()
    {
        for (var directory = new DirectoryInfo(AppContext.BaseDirectory); directory is not null; directory = directory.Parent)
        {
            if (File.Exists(Path.Combine(directory.FullName, "proto", "dq.proto")))
            {
                var name = OperatingSystem.IsWindows() ? "dq.exe" : "dq";
                return Path.Combine(directory.FullName, "target", "debug", name);
            }
        }

        throw new InvalidOperationException("Could not find the DQ repository. Set DQ_SERVER_BIN to the Rust server binary.");
    }
}
