#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NegotiateRequest {
    #[prost(oneof="negotiate_request::Which", tags="1, 2, 3")]
    pub which: ::core::option::Option<negotiate_request::Which>,
}
/// Nested message and enum types in `NegotiateRequest`.
pub mod negotiate_request {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Start {
        #[prost(string, tag="1")]
        pub session_id: ::prost::alloc::string::String,
        #[prost(string, tag="2")]
        pub offer_sdp: ::prost::alloc::string::String,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Answer {
        #[prost(string, tag="1")]
        pub sdp: ::prost::alloc::string::String,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct IceCandidate {
        #[prost(string, tag="1")]
        pub ice_candidate: ::prost::alloc::string::String,
    }
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Which {
        #[prost(message, tag="1")]
        Start(Start),
        #[prost(message, tag="2")]
        Answer(Answer),
        #[prost(message, tag="3")]
        IceCandidate(IceCandidate),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NegotiateResponse {
    #[prost(oneof="negotiate_response::Which", tags="1, 2, 3")]
    pub which: ::core::option::Option<negotiate_response::Which>,
}
/// Nested message and enum types in `NegotiateResponse`.
pub mod negotiate_response {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Offer {
        #[prost(string, tag="1")]
        pub sdp: ::prost::alloc::string::String,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Answer {
        #[prost(string, tag="1")]
        pub sdp: ::prost::alloc::string::String,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct IceCandidate {
        #[prost(string, tag="1")]
        pub ice_candidate: ::prost::alloc::string::String,
    }
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Which {
        #[prost(message, tag="1")]
        Offer(Offer),
        #[prost(message, tag="2")]
        Answer(Answer),
        #[prost(message, tag="3")]
        IceCandidate(IceCandidate),
    }
}
const METHOD_SESSION_SERVICE_NEGOTIATE: ::grpcio::Method<NegotiateRequest, NegotiateResponse> = ::grpcio::Method{ty: ::grpcio::MethodType::Duplex, name: "/signor.SessionService/Negotiate", req_mar: ::grpcio::Marshaller { ser: ::grpcio::pr_ser, de: ::grpcio::pr_de }, resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pr_ser, de: ::grpcio::pr_de }, };
#[derive(Clone)]
pub struct SessionServiceClient { client: ::grpcio::Client }
impl SessionServiceClient {
pub fn new(channel: ::grpcio::Channel) -> Self { SessionServiceClient { client: ::grpcio::Client::new(channel) }}
pub fn negotiate_opt(&self, opt: ::grpcio::CallOption) -> ::grpcio::Result<(::grpcio::ClientDuplexSender<NegotiateRequest>,::grpcio::ClientDuplexReceiver<NegotiateResponse>,)> { self.client.duplex_streaming(&METHOD_SESSION_SERVICE_NEGOTIATE, opt) }
pub fn negotiate(&self) -> ::grpcio::Result<(::grpcio::ClientDuplexSender<NegotiateRequest>,::grpcio::ClientDuplexReceiver<NegotiateResponse>,)> { self.negotiate_opt(::grpcio::CallOption::default()) }
pub fn spawn<F>(&self, f: F) where F: ::std::future::Future<Output = ()> + Send + 'static {self.client.spawn(f)}
}
pub trait SessionService {
fn negotiate(&mut self, ctx: ::grpcio::RpcContext, _stream: ::grpcio::RequestStream<NegotiateRequest>, sink: ::grpcio::DuplexSink<NegotiateResponse>) { grpcio::unimplemented_call!(ctx, sink) }
}
pub fn create_session_service<S: SessionService + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
let mut builder = ::grpcio::ServiceBuilder::new();
let mut instance = s;
builder = builder.add_duplex_streaming_handler(&METHOD_SESSION_SERVICE_NEGOTIATE, move |ctx, req, resp| instance.negotiate(ctx, req, resp));
builder.build()
}
