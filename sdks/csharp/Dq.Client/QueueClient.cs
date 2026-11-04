using Dq.V1;
using Google.Protobuf;
using Grpc.Core;
using Grpc.Net.Client;

namespace Dq.Client;

public sealed class QueueClient : IDisposable
{
    private readonly GrpcChannel _channel;
    private readonly QueueService.QueueServiceClient _queue;
    private readonly TimeSpan _requestTimeout;

    public QueueClient(string address = "http://127.0.0.1:50051", TimeSpan? requestTimeout = null)
    {
        _channel = GrpcChannel.ForAddress(address);
        _queue = new QueueService.QueueServiceClient(_channel);
        _requestTimeout = requestTimeout ?? TimeSpan.FromSeconds(5);
    }

    public async Task<string> EnqueueAsync(
        string serviceName,
        string functionName,
        string submissionKey,
        ByteString arguments,
        string caller = "",
        CancellationToken cancellationToken = default)
    {
        var request = new EnqueueRequest
        {
            ServiceName = serviceName,
            FunctionName = functionName,
            SubmissionKey = submissionKey,
            Arguments = arguments,
            Caller = caller,
        };
        using var call = _queue.EnqueueAsync(request, deadline: Deadline(), cancellationToken: cancellationToken);
        return (await call.ResponseAsync.ConfigureAwait(false)).JobId;
    }

    public async Task<GetJobStatusResponse> GetJobStatusAsync(
        string jobId, CancellationToken cancellationToken = default)
    {
        using var call = _queue.GetJobStatusAsync(
            new GetJobStatusRequest { JobId = jobId },
            deadline: Deadline(), cancellationToken: cancellationToken);
        return await call.ResponseAsync.ConfigureAwait(false);
    }

    public AsyncServerStreamingCall<ExecuteRequest> SubscribeExecution(
        string serviceName, IEnumerable<string> functionNames, CancellationToken cancellationToken = default) =>
        _queue.SubscribeExecution(Subscription(serviceName, functionNames), cancellationToken: cancellationToken);

    public async Task<AsyncServerStreamingCall<ExecuteResponse>> SubscribeCompletionAsync(
        string serviceName, IEnumerable<string> functionNames, CancellationToken cancellationToken = default)
    {
        var call = _queue.SubscribeCompletion(
            Subscription(serviceName, functionNames), cancellationToken: cancellationToken);
        try
        {
            // Wait for registration so the caller can submit immediately without missing the outcome.
            await call.ResponseHeadersAsync.ConfigureAwait(false);
            return call;
        }
        catch
        {
            call.Dispose();
            throw;
        }
    }

    public Task CompleteAsync(ExecuteRequest assignment, ByteString result, CancellationToken cancellationToken = default) =>
        ReportOutcomeAsync(assignment, new CompleteExecutionRequest { Result = result }, cancellationToken);

    public Task FailAsync(ExecuteRequest assignment, string error, CancellationToken cancellationToken = default) =>
        ReportOutcomeAsync(assignment, new CompleteExecutionRequest { Error = error }, cancellationToken);

    public void Dispose() => _channel.Dispose();

    private async Task ReportOutcomeAsync(
        ExecuteRequest assignment, CompleteExecutionRequest request, CancellationToken cancellationToken)
    {
        request.JobId = assignment.JobId;
        request.AttemptId = assignment.AttemptId;
        using var call = _queue.CompleteExecutionAsync(request, deadline: Deadline(), cancellationToken: cancellationToken);
        await call.ResponseAsync.ConfigureAwait(false);
    }

    private DateTime Deadline() => DateTime.UtcNow + _requestTimeout;

    private static SubscribeRequest Subscription(string serviceName, IEnumerable<string> functionNames) =>
        new() { ServiceName = serviceName, FunctionNames = { functionNames } };
}
