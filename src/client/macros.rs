#[macro_export]
macro_rules! register_client {
    (
        $(($module:ident, $name:literal, $config:ident, $client:ident),)+
    ) => {
        $(
            mod $module;
        )+
        $(
            use self::$module::$config;
        )+

        #[derive(Debug, Clone, serde::Deserialize)]
        #[serde(tag = "type")]
        pub enum ClientConfig {
            $(
                #[serde(rename = $name)]
                $config($config),
            )+
            #[serde(other)]
            Unknown,
        }

        $(
            #[derive(Debug)]
            pub struct $client {
                global_config: $crate::config::GlobalConfig,
                config: $config,
                model: $crate::client::Model,
            }

            impl $client {
                pub const NAME: &'static str = $name;

                pub fn init(global_config: &$crate::config::GlobalConfig, model: &$crate::client::Model) -> Option<Box<dyn Client>> {
                    let config = global_config.read().clients.iter().find_map(|client_config| {
                        if let ClientConfig::$config(c) = client_config {
                            if Self::name(c) == model.client_name() {
                                return Some(c.clone())
                            }
                        }
                        None
                    })?;

                    Some(Box::new(Self {
                        global_config: global_config.clone(),
                        config,
                        model: model.clone(),
                    }))
                }

                pub fn list_models(local_config: &$config) -> Vec<Model> {
                    let client_name = Self::name(local_config);
                    if local_config.models.is_empty() {
                        if let Some(models) = $crate::client::ALL_MODELS.iter().find(|v| {
                            v.platform == $name ||
                                ($name == OpenAICompatibleClient::NAME && local_config.name.as_deref() == Some(&v.platform))
                        }) {
                            return Model::from_config(client_name, &models.models);
                        }
                        vec![]
                    } else {
                        Model::from_config(client_name, &local_config.models)
                    }
                }

                pub fn name(local_config: &$config) -> &str {
                    local_config.name.as_deref().unwrap_or(Self::NAME)
                }
            }

        )+

        pub fn init_client(config: &$crate::config::GlobalConfig, model: Option<$crate::client::Model>) -> anyhow::Result<Box<dyn Client>> {
            let model = model.unwrap_or_else(|| config.read().model.clone());
            None
            $(.or_else(|| $client::init(config, &model)))+
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid model '{}'", model.id())
            })
        }

        pub fn list_client_types() -> Vec<&'static str> {
            let mut client_types: Vec<_> = vec![$($client::NAME,)+];
            client_types.extend($crate::client::OPENAI_COMPATIBLE_PLATFORMS.iter().map(|(name, _)| *name));
            client_types
        }

        pub fn create_client_config(client: &str) -> anyhow::Result<(String, serde_json::Value)> {
            $(
                if client == $client::NAME {
                    return create_config(&$client::PROMPTS, $client::NAME)
                }
            )+
            if let Some(ret) = create_openai_compatible_client_config(client)? {
                return Ok(ret);
            }
            anyhow::bail!("Unknown client '{}'", client)
        }

        static mut ALL_CLIENT_MODELS: Option<Vec<$crate::client::Model>> = None;

        pub fn list_models(config: &$crate::config::Config) -> Vec<&'static $crate::client::Model> {
            if unsafe { ALL_CLIENT_MODELS.is_none() } {
                let models: Vec<_> = config
                    .clients
                    .iter()
                    .flat_map(|v| match v {
                        $(ClientConfig::$config(c) => $client::list_models(c),)+
                        ClientConfig::Unknown => vec![],
                    })
                    .collect();
                unsafe { ALL_CLIENT_MODELS = Some(models) };
            }
            unsafe { ALL_CLIENT_MODELS.as_ref().unwrap().iter().collect() }
        }

        pub fn list_chat_models(config: &$crate::config::Config) -> Vec<&'static $crate::client::Model> {
            list_models(config).into_iter().filter(|v| v.model_type() == "chat").collect()
        }

        pub fn list_embedding_models(config: &$crate::config::Config) -> Vec<&'static $crate::client::Model> {
            list_models(config).into_iter().filter(|v| v.model_type() == "embedding").collect()
        }

        pub fn list_reranker_models(config: &$crate::config::Config) -> Vec<&'static $crate::client::Model> {
            list_models(config).into_iter().filter(|v| v.model_type() == "reranker").collect()
        }
    };
}

#[macro_export]
macro_rules! client_common_fns {
    () => {
        fn global_config(&self) -> &$crate::config::GlobalConfig {
            &self.global_config
        }

        fn extra_config(&self) -> Option<&$crate::client::ExtraConfig> {
            self.config.extra.as_ref()
        }

        fn patch_config(&self) -> Option<&$crate::client::RequestPatch> {
            self.config.patch.as_ref()
        }

        fn name(&self) -> &str {
            Self::name(&self.config)
        }

        fn model(&self) -> &Model {
            &self.model
        }

        fn model_mut(&mut self) -> &mut Model {
            &mut self.model
        }
    };
}

#[macro_export]
macro_rules! impl_client_trait {
    (
        $client:ident,
        ($prepare_chat_completions:path, $chat_completions:path, $chat_completions_streaming:path),
        ($prepare_embeddings:path, $embeddings:path),
        ($prepare_rerank:path, $rerank:path),
    ) => {
        #[async_trait::async_trait]
        impl $crate::client::Client for $crate::client::$client {
            client_common_fns!();

            async fn chat_completions_inner(
                &self,
                client: &reqwest::Client,
                data: $crate::client::ChatCompletionsData,
            ) -> anyhow::Result<$crate::client::ChatCompletionsOutput> {
                let request_data = $prepare_chat_completions(self, data)?;
                let builder = self.request_builder(client, request_data, ApiType::ChatCompletions);
                $chat_completions(builder, self.model()).await
            }

            async fn chat_completions_streaming_inner(
                &self,
                client: &reqwest::Client,
                handler: &mut $crate::client::SseHandler,
                data: $crate::client::ChatCompletionsData,
            ) -> Result<()> {
                let request_data = $prepare_chat_completions(self, data)?;
                let builder = self.request_builder(client, request_data, ApiType::ChatCompletions);
                $chat_completions_streaming(builder, handler, self.model()).await
            }

            async fn embeddings_inner(
                &self,
                client: &reqwest::Client,
                data: &$crate::client::EmbeddingsData,
            ) -> Result<$crate::client::EmbeddingsOutput> {
                let request_data = $prepare_embeddings(self, data)?;
                let builder = self.request_builder(client, request_data, ApiType::Embeddings);
                $embeddings(builder, self.model()).await
            }

            async fn rerank_inner(
                &self,
                client: &reqwest::Client,
                data: &$crate::client::RerankData,
            ) -> Result<$crate::client::RerankOutput> {
                let request_data = $prepare_rerank(self, data)?;
                let builder = self.request_builder(client, request_data, ApiType::Rerank);
                $rerank(builder, self.model()).await
            }
        }
    };
}

#[macro_export]
macro_rules! config_get_fn {
    ($field_name:ident, $fn_name:ident) => {
        fn $fn_name(&self) -> anyhow::Result<String> {
            let api_key = self.config.$field_name.clone();
            api_key
                .or_else(|| {
                    let env_prefix = Self::name(&self.config);
                    let env_name =
                        format!("{}_{}", env_prefix, stringify!($field_name)).to_ascii_uppercase();
                    std::env::var(&env_name).ok()
                })
                .ok_or_else(|| anyhow::anyhow!("Miss '{}'", stringify!($field_name)))
        }
    };
}

#[macro_export]
macro_rules! unsupported_model {
    ($name:expr) => {
        anyhow::bail!("Unsupported model '{}'", $name)
    };
}
