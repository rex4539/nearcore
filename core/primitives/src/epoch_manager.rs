use crate::num_rational::Rational32;
use crate::shard_layout::ShardLayout;
use crate::types::validator_stake::ValidatorStake;
use crate::types::{
    AccountId, Balance, BlockChunkValidatorStats, BlockHeightDelta, NumSeats, ProtocolVersion,
    ValidatorKickoutReason,
};
use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives_core::checked_feature;
use near_primitives_core::hash::CryptoHash;
use near_primitives_core::serialize::dec_format;
use near_primitives_core::version::{ProtocolFeature, PROTOCOL_VERSION};
use near_schema_checker_lib::ProtocolSchema;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::Arc;

pub const AGGREGATOR_KEY: &[u8] = b"AGGREGATOR";

/// Epoch config, determines validator assignment for given epoch.
/// Can change from epoch to epoch depending on the sharding and other parameters, etc.
#[derive(Clone, Eq, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EpochConfig {
    /// Epoch length in block heights.
    pub epoch_length: BlockHeightDelta,
    /// Number of seats for block producers.
    pub num_block_producer_seats: NumSeats,
    /// Number of seats of block producers per each shard.
    pub num_block_producer_seats_per_shard: Vec<NumSeats>,
    /// Expected number of hidden validator seats per each shard.
    pub avg_hidden_validator_seats_per_shard: Vec<NumSeats>,
    /// Threshold for kicking out block producers.
    pub block_producer_kickout_threshold: u8,
    /// Threshold for kicking out chunk producers.
    pub chunk_producer_kickout_threshold: u8,
    /// Threshold for kicking out nodes which are only chunk validators.
    pub chunk_validator_only_kickout_threshold: u8,
    /// Number of target chunk validator mandates for each shard.
    pub target_validator_mandates_per_shard: NumSeats,
    /// Max ratio of validators that we can kick out in an epoch
    pub validator_max_kickout_stake_perc: u8,
    /// Online minimum threshold below which validator doesn't receive reward.
    pub online_min_threshold: Rational32,
    /// Online maximum threshold above which validator gets full reward.
    pub online_max_threshold: Rational32,
    /// Stake threshold for becoming a fisherman.
    #[serde(with = "dec_format")]
    pub fishermen_threshold: Balance,
    /// The minimum stake required for staking is last seat price divided by this number.
    pub minimum_stake_divisor: u64,
    /// Threshold of stake that needs to indicate that they ready for upgrade.
    pub protocol_upgrade_stake_threshold: Rational32,
    /// Shard layout of this epoch, may change from epoch to epoch
    pub shard_layout: ShardLayout,
    /// Additional configuration parameters for the new validator selection
    /// algorithm. See <https://github.com/near/NEPs/pull/167> for details.
    // #[default(100)]
    pub num_chunk_producer_seats: NumSeats,
    // #[default(300)]
    pub num_chunk_validator_seats: NumSeats,
    // TODO (#11267): deprecate after StatelessValidationV0 is in place.
    // Use 300 for older protocol versions.
    // #[default(300)]
    pub num_chunk_only_producer_seats: NumSeats,
    // #[default(1)]
    pub minimum_validators_per_shard: NumSeats,
    // #[default(Rational32::new(160, 1_000_000))]
    pub minimum_stake_ratio: Rational32,
    // #[default(5)]
    /// Limits the number of shard changes in chunk producer assignments,
    /// if algorithm is able to choose assignment with better balance of
    /// number of chunk producers for shards.
    pub chunk_producer_assignment_changes_limit: NumSeats,
    // #[default(false)]
    pub shuffle_shard_assignment_for_chunk_producers: bool,
}

impl EpochConfig {
    /// Total number of validator seats in the epoch since protocol version 69.
    pub fn num_validators(&self) -> NumSeats {
        self.num_block_producer_seats
            .max(self.num_chunk_producer_seats)
            .max(self.num_chunk_validator_seats)
    }
}

impl EpochConfig {
    // Create test-only epoch config.
    // Not depends on genesis!
    pub fn genesis_test(
        num_block_producer_seats: NumSeats,
        shard_layout: ShardLayout,
        epoch_length: BlockHeightDelta,
        block_producer_kickout_threshold: u8,
        chunk_producer_kickout_threshold: u8,
        chunk_validator_only_kickout_threshold: u8,
        protocol_upgrade_stake_threshold: Rational32,
        fishermen_threshold: Balance,
    ) -> Self {
        Self {
            epoch_length,
            num_block_producer_seats,
            num_block_producer_seats_per_shard: vec![
                num_block_producer_seats;
                shard_layout.shard_ids().count()
            ],
            avg_hidden_validator_seats_per_shard: vec![],
            target_validator_mandates_per_shard: 68,
            validator_max_kickout_stake_perc: 100,
            online_min_threshold: Rational32::new(90, 100),
            online_max_threshold: Rational32::new(99, 100),
            minimum_stake_divisor: 10,
            protocol_upgrade_stake_threshold,
            block_producer_kickout_threshold,
            chunk_producer_kickout_threshold,
            chunk_validator_only_kickout_threshold,
            fishermen_threshold,
            shard_layout,
            num_chunk_producer_seats: 100,
            num_chunk_validator_seats: 300,
            num_chunk_only_producer_seats: 300,
            minimum_validators_per_shard: 1,
            minimum_stake_ratio: Rational32::new(160i32, 1_000_000i32),
            chunk_producer_assignment_changes_limit: 5,
            shuffle_shard_assignment_for_chunk_producers: false,
        }
    }

    /// Mignimal config for testing.
    pub fn minimal() -> Self {
        Self {
            epoch_length: 0,
            num_block_producer_seats: 0,
            num_block_producer_seats_per_shard: vec![],
            avg_hidden_validator_seats_per_shard: vec![],
            block_producer_kickout_threshold: 0,
            chunk_producer_kickout_threshold: 0,
            chunk_validator_only_kickout_threshold: 0,
            target_validator_mandates_per_shard: 0,
            validator_max_kickout_stake_perc: 0,
            online_min_threshold: 0.into(),
            online_max_threshold: 0.into(),
            fishermen_threshold: 0,
            minimum_stake_divisor: 0,
            protocol_upgrade_stake_threshold: 0.into(),
            shard_layout: ShardLayout::get_simple_nightshade_layout(),
            num_chunk_producer_seats: 100,
            num_chunk_validator_seats: 300,
            num_chunk_only_producer_seats: 300,
            minimum_validators_per_shard: 1,
            minimum_stake_ratio: Rational32::new(160i32, 1_000_000i32),
            chunk_producer_assignment_changes_limit: 5,
            shuffle_shard_assignment_for_chunk_producers: false,
        }
    }

    pub fn mock(epoch_length: BlockHeightDelta, shard_layout: ShardLayout) -> Self {
        Self {
            epoch_length,
            num_block_producer_seats: 2,
            num_block_producer_seats_per_shard: vec![1, 1],
            avg_hidden_validator_seats_per_shard: vec![1, 1],
            block_producer_kickout_threshold: 0,
            chunk_producer_kickout_threshold: 0,
            chunk_validator_only_kickout_threshold: 0,
            target_validator_mandates_per_shard: 1,
            validator_max_kickout_stake_perc: 0,
            online_min_threshold: Rational32::new(1i32, 4i32),
            online_max_threshold: Rational32::new(3i32, 4i32),
            fishermen_threshold: 1,
            minimum_stake_divisor: 1,
            protocol_upgrade_stake_threshold: Rational32::new(3i32, 4i32),
            shard_layout,
            num_chunk_producer_seats: 100,
            num_chunk_validator_seats: 300,
            num_chunk_only_producer_seats: 300,
            minimum_validators_per_shard: 1,
            minimum_stake_ratio: Rational32::new(160i32, 1_000_000i32),
            chunk_producer_assignment_changes_limit: 5,
            shuffle_shard_assignment_for_chunk_producers: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShardConfig {
    pub num_block_producer_seats_per_shard: Vec<NumSeats>,
    pub avg_hidden_validator_seats_per_shard: Vec<NumSeats>,
    pub shard_layout: ShardLayout,
}

impl ShardConfig {
    pub fn new(epoch_config: EpochConfig) -> Self {
        Self {
            num_block_producer_seats_per_shard: epoch_config
                .num_block_producer_seats_per_shard
                .clone(),
            avg_hidden_validator_seats_per_shard: epoch_config
                .avg_hidden_validator_seats_per_shard
                .clone(),
            shard_layout: epoch_config.shard_layout,
        }
    }
}

/// Testing overrides to apply to the EpochConfig returned by the `for_protocol_version`.
/// All fields should be optional and the default should be a no-op.
#[derive(Clone, Debug, Default)]
pub struct AllEpochConfigTestOverrides {
    pub block_producer_kickout_threshold: Option<u8>,
    pub chunk_producer_kickout_threshold: Option<u8>,
}

/// AllEpochConfig manages protocol configs that might be changing throughout epochs (hence EpochConfig).
/// The main function in AllEpochConfig is ::for_protocol_version which takes a protocol version
/// and returns the EpochConfig that should be used for this protocol version.
#[derive(Debug, Clone)]
pub struct AllEpochConfig {
    /// Store for EpochConfigs, provides configs per protocol version.
    /// Initialized only for production, ie. when `use_protocol_version` is true.
    config_store: Option<EpochConfigStore>,
    /// Chain Id. Some parameters are specific to certain chains.
    chain_id: String,
    epoch_length: BlockHeightDelta,
    /// The fields below are DEPRECATED.
    /// Epoch config must be controlled by `config_store` only.
    /// TODO(#11265): remove these fields.
    /// Whether this is for production (i.e., mainnet or testnet). This is a temporary implementation
    /// to allow us to change protocol config for mainnet and testnet without changing the genesis config
    _use_production_config: bool,
    /// EpochConfig from genesis
    _genesis_epoch_config: EpochConfig,

    /// Testing overrides to apply to the EpochConfig returned by the `for_protocol_version`.
    _test_overrides: AllEpochConfigTestOverrides,
}

impl AllEpochConfig {
    pub fn new(
        use_production_config: bool,
        genesis_protocol_version: ProtocolVersion,
        genesis_epoch_config: EpochConfig,
        chain_id: &str,
    ) -> Self {
        Self::new_with_test_overrides(
            use_production_config,
            genesis_protocol_version,
            genesis_epoch_config,
            chain_id,
            Some(AllEpochConfigTestOverrides::default()),
        )
    }

    pub fn from_epoch_config_store(
        chain_id: &str,
        epoch_length: BlockHeightDelta,
        epoch_config_store: EpochConfigStore,
    ) -> Self {
        let genesis_epoch_config = epoch_config_store.get_config(PROTOCOL_VERSION).as_ref().clone();
        Self {
            config_store: Some(epoch_config_store),
            chain_id: chain_id.to_string(),
            epoch_length,
            // The fields below must be DEPRECATED. Don't use it for epoch
            // config creation.
            // TODO(#11265): remove them.
            _use_production_config: false,
            _genesis_epoch_config: genesis_epoch_config,
            _test_overrides: AllEpochConfigTestOverrides::default(),
        }
    }

    /// DEPRECATED.
    pub fn new_with_test_overrides(
        use_production_config: bool,
        genesis_protocol_version: ProtocolVersion,
        genesis_epoch_config: EpochConfig,
        chain_id: &str,
        test_overrides: Option<AllEpochConfigTestOverrides>,
    ) -> Self {
        // Use the config store only for production configs and outside of tests.
        let config_store = if use_production_config && test_overrides.is_none() {
            EpochConfigStore::for_chain_id(chain_id, None)
        } else {
            None
        };
        let all_epoch_config = Self {
            config_store: config_store.clone(),
            chain_id: chain_id.to_string(),
            epoch_length: genesis_epoch_config.epoch_length,
            _use_production_config: use_production_config,
            _genesis_epoch_config: genesis_epoch_config,
            _test_overrides: test_overrides.unwrap_or_default(),
        };
        // Sanity check: Validate that the stored genesis config equals to the config generated for the genesis protocol version.
        // Note that we cannot do this in unittests because we do not have direct access to the genesis config for mainnet/testnet.
        // Thus, by making sure that the generated and store configs match for the genesis, we complement the unittests, which
        // check that the generated and stored configs match for the versions after the genesis.
        if config_store.is_some() {
            debug_assert_eq!(
                config_store.as_ref().unwrap().get_config(genesis_protocol_version).as_ref(),
                &all_epoch_config.generate_epoch_config(genesis_protocol_version),
                "Provided genesis EpochConfig for protocol version {} does not match the stored config", genesis_protocol_version
            );
        }
        all_epoch_config
    }

    pub fn for_protocol_version(&self, protocol_version: ProtocolVersion) -> EpochConfig {
        if self.config_store.is_some() {
            let mut config =
                self.config_store.as_ref().unwrap().get_config(protocol_version).as_ref().clone();
            // TODO(#11265): epoch length is overridden in many tests so we
            // need to support it here. Consider removing `epoch_length` from
            // EpochConfig.
            config.epoch_length = self.epoch_length;
            config
        } else {
            self.generate_epoch_config(protocol_version)
        }
    }

    /// TODO(#11265): Remove this and use the stored configs only.
    pub fn generate_epoch_config(&self, protocol_version: ProtocolVersion) -> EpochConfig {
        let mut config = self._genesis_epoch_config.clone();

        Self::config_mocknet(&mut config, &self.chain_id);

        if !self._use_production_config {
            return config;
        }

        Self::config_validator_selection(&mut config, protocol_version);

        Self::config_nightshade(&mut config, protocol_version);

        Self::config_chunk_only_producers(&mut config, &self.chain_id, protocol_version);

        Self::config_max_kickout_stake(&mut config, protocol_version);

        Self::config_fix_min_stake_ratio(&mut config, protocol_version);

        Self::config_chunk_endorsement_thresholds(&mut config, protocol_version);

        Self::config_test_overrides(&mut config, &self._test_overrides);

        config
    }

    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    /// Configures mocknet-specific features only.
    fn config_mocknet(config: &mut EpochConfig, chain_id: &str) {
        if chain_id != near_primitives_core::chains::MOCKNET {
            return;
        }
        // In production (mainnet/testnet) and nightly environments this setting is guarded by
        // ProtocolFeature::ShuffleShardAssignments. (see config_validator_selection function).
        // For pre-release environment such as mocknet, which uses features between production and nightly
        // (eg. stateless validation) we enable it by default with stateless validation in order to exercise
        // the codepaths for state sync more often.
        // TODO(#11201): When stabilizing "ShuffleShardAssignments" in mainnet,
        // also remove this temporary code and always rely on ShuffleShardAssignments.
        config.shuffle_shard_assignment_for_chunk_producers = true;
    }

    /// Configures validator-selection related features.
    fn config_validator_selection(config: &mut EpochConfig, protocol_version: ProtocolVersion) {
        // Shuffle shard assignments every epoch, to trigger state sync more
        // frequently to exercise that code path.
        if checked_feature!("stable", ShuffleShardAssignments, protocol_version) {
            config.shuffle_shard_assignment_for_chunk_producers = true;
        }
    }

    fn config_nightshade(config: &mut EpochConfig, protocol_version: ProtocolVersion) {
        // Unlike the other checks, this one is for strict equality. The testonly nightshade layout
        // is specifically used in resharding tests, not for any other protocol versions.
        #[cfg(feature = "nightly")]
        if protocol_version == ProtocolFeature::SimpleNightshadeTestonly.protocol_version() {
            Self::config_nightshade_impl(
                config,
                ShardLayout::get_simple_nightshade_layout_testonly(),
            );
            return;
        }

        if checked_feature!("stable", SimpleNightshadeV3, protocol_version) {
            Self::config_nightshade_impl(config, ShardLayout::get_simple_nightshade_layout_v3());
            return;
        }

        if checked_feature!("stable", SimpleNightshadeV2, protocol_version) {
            Self::config_nightshade_impl(config, ShardLayout::get_simple_nightshade_layout_v2());
            return;
        }

        if checked_feature!("stable", SimpleNightshade, protocol_version) {
            Self::config_nightshade_impl(config, ShardLayout::get_simple_nightshade_layout());
            return;
        }
    }

    fn config_nightshade_impl(config: &mut EpochConfig, shard_layout: ShardLayout) {
        let num_block_producer_seats = config.num_block_producer_seats;
        config.num_block_producer_seats_per_shard =
            shard_layout.shard_ids().map(|_| num_block_producer_seats).collect();
        config.avg_hidden_validator_seats_per_shard = shard_layout.shard_ids().map(|_| 0).collect();
        config.shard_layout = shard_layout;
    }

    fn config_chunk_only_producers(
        config: &mut EpochConfig,
        chain_id: &str,
        protocol_version: u32,
    ) {
        if checked_feature!("stable", ChunkOnlyProducers, protocol_version) {
            // On testnet, genesis config set num_block_producer_seats to 200
            // This is to bring it back to 100 to be the same as on mainnet
            config.num_block_producer_seats = 100;
            // Technically, after ChunkOnlyProducers is enabled, this field is no longer used
            // We still set it here just in case
            config.num_block_producer_seats_per_shard =
                config.shard_layout.shard_ids().map(|_| 100).collect();
            config.block_producer_kickout_threshold = 80;
            config.chunk_producer_kickout_threshold = 80;
            config.num_chunk_only_producer_seats = 200;
        }

        // Adjust the number of block and chunk producers for testnet, to make it easier to test the change.
        if chain_id == near_primitives_core::chains::TESTNET
            && checked_feature!("stable", TestnetFewerBlockProducers, protocol_version)
        {
            let shard_ids = config.shard_layout.shard_ids();
            // Decrease the number of block and chunk producers from 100 to 20.
            config.num_block_producer_seats = 20;
            // Checking feature NoChunkOnlyProducers in stateless validation
            if ProtocolFeature::StatelessValidation.enabled(protocol_version) {
                config.num_chunk_producer_seats = 20;
            }
            config.num_block_producer_seats_per_shard =
                shard_ids.map(|_| config.num_block_producer_seats).collect();
            // Decrease the number of chunk producers.
            config.num_chunk_only_producer_seats = 100;
        }

        // Checking feature NoChunkOnlyProducers in stateless validation
        if ProtocolFeature::StatelessValidation.enabled(protocol_version) {
            // Make sure there is no chunk only producer in stateless validation
            config.num_chunk_only_producer_seats = 0;
        }
    }

    fn config_max_kickout_stake(config: &mut EpochConfig, protocol_version: u32) {
        if checked_feature!("stable", MaxKickoutStake, protocol_version) {
            config.validator_max_kickout_stake_perc = 30;
        }
    }

    fn config_fix_min_stake_ratio(config: &mut EpochConfig, protocol_version: u32) {
        if checked_feature!("stable", FixMinStakeRatio, protocol_version) {
            config.minimum_stake_ratio = Rational32::new(1, 62_500);
        }
    }

    fn config_chunk_endorsement_thresholds(config: &mut EpochConfig, protocol_version: u32) {
        if ProtocolFeature::ChunkEndorsementsInBlockHeader.enabled(protocol_version) {
            config.chunk_validator_only_kickout_threshold = 70;
        }
    }

    fn config_test_overrides(
        config: &mut EpochConfig,
        test_overrides: &AllEpochConfigTestOverrides,
    ) {
        if let Some(block_producer_kickout_threshold) =
            test_overrides.block_producer_kickout_threshold
        {
            config.block_producer_kickout_threshold = block_producer_kickout_threshold;
        }

        if let Some(chunk_producer_kickout_threshold) =
            test_overrides.chunk_producer_kickout_threshold
        {
            config.chunk_producer_kickout_threshold = chunk_producer_kickout_threshold;
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, ProtocolSchema)]
pub struct EpochSummary {
    pub prev_epoch_last_block_hash: CryptoHash,
    /// Proposals from the epoch, only the latest one per account
    pub all_proposals: Vec<ValidatorStake>,
    /// Kickout set, includes slashed
    pub validator_kickout: HashMap<AccountId, ValidatorKickoutReason>,
    /// Only for validators who met the threshold and didn't get slashed
    pub validator_block_chunk_stats: HashMap<AccountId, BlockChunkValidatorStats>,
    /// Protocol version for next next epoch, as summary of epoch T defines
    /// epoch T+2.
    pub next_next_epoch_version: ProtocolVersion,
}

macro_rules! include_config {
    ($chain:expr, $version:expr, $file:expr) => {
        (
            $chain,
            $version,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/res/epoch_configs/",
                $chain,
                "/",
                $file
            )),
        )
    };
}

/// List of (chain_id, version, JSON content) tuples used to initialize the EpochConfigStore.
static CONFIGS: &[(&str, ProtocolVersion, &str)] = &[
    // Epoch configs for mainnet (genesis protool version is 29).
    include_config!("mainnet", 29, "29.json"),
    include_config!("mainnet", 48, "48.json"),
    include_config!("mainnet", 56, "56.json"),
    include_config!("mainnet", 64, "64.json"),
    include_config!("mainnet", 65, "65.json"),
    include_config!("mainnet", 69, "69.json"),
    include_config!("mainnet", 70, "70.json"),
    include_config!("mainnet", 71, "71.json"),
    include_config!("mainnet", 72, "72.json"),
    include_config!("mainnet", 100, "100.json"),
    include_config!("mainnet", 101, "101.json"),
    include_config!("mainnet", 143, "143.json"),
    // Epoch configs for testnet (genesis protool version is 29).
    include_config!("testnet", 29, "29.json"),
    include_config!("testnet", 48, "48.json"),
    include_config!("testnet", 56, "56.json"),
    include_config!("testnet", 64, "64.json"),
    include_config!("testnet", 65, "65.json"),
    include_config!("testnet", 69, "69.json"),
    include_config!("testnet", 70, "70.json"),
    include_config!("testnet", 71, "71.json"),
    include_config!("testnet", 72, "72.json"),
    include_config!("testnet", 100, "100.json"),
    include_config!("testnet", 101, "101.json"),
    include_config!("testnet", 143, "143.json"),
];

/// Store for `[EpochConfig]` per protocol version.`
#[derive(Debug, Clone)]
pub struct EpochConfigStore {
    store: BTreeMap<ProtocolVersion, Arc<EpochConfig>>,
}

impl EpochConfigStore {
    /// Creates a config store to contain the EpochConfigs for the given chain parsed from the JSON files.
    /// If no configs are found for the given chain, try to load the configs from the file system.
    /// If there are no configs found, return None.
    pub fn for_chain_id(chain_id: &str, config_dir: Option<PathBuf>) -> Option<Self> {
        let mut store = Self::load_default_epoch_configs(chain_id);

        if !store.is_empty() {
            return Some(Self { store });
        }
        if let Some(config_dir) = config_dir {
            store = Self::load_epoch_config_from_file_system(config_dir.to_str().unwrap());
        }

        if store.is_empty() {
            None
        } else {
            Some(Self { store })
        }
    }

    /// Loads the default epoch configs for the given chain from the CONFIGS array.
    fn load_default_epoch_configs(chain_id: &str) -> BTreeMap<ProtocolVersion, Arc<EpochConfig>> {
        let mut store = BTreeMap::new();
        for (chain, version, content) in CONFIGS.iter() {
            if *chain == chain_id {
                let config: EpochConfig = serde_json::from_str(*content).unwrap_or_else(|e| {
                    panic!(
                        "Failed to load epoch config files for chain {} and version {}: {:#}",
                        chain_id, version, e
                    )
                });
                store.insert(*version, Arc::new(config));
            }
        }
        store
    }

    /// Reads the json files from the epoch config directory.
    fn load_epoch_config_from_file_system(
        directory: &str,
    ) -> BTreeMap<ProtocolVersion, Arc<EpochConfig>> {
        let mut store = BTreeMap::new();
        let entries = fs::read_dir(directory).expect("Failed opening epoch config directory");

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();

                // Check if the file has a .json extension
                if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                    // Extract the file name (without extension)
                    if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let version: ProtocolVersion =
                            file_stem.parse().expect("Invalid protocol version");

                        let content = fs::read_to_string(&path).expect("Failed to read file");
                        let config: EpochConfig =
                            serde_json::from_str(&content).unwrap_or_else(|e| {
                                panic!(
                                "Failed to load epoch config from file system for version {}: {:#}",
                                version, e
                            )
                            });

                        store.insert(version, Arc::new(config));
                    }
                }
            }
        }

        store
    }

    pub fn test(store: BTreeMap<ProtocolVersion, Arc<EpochConfig>>) -> Self {
        Self { store }
    }

    /// Returns the EpochConfig for the given protocol version.
    /// This panics if no config is found for the given version, thus the initialization via `for_chain_id` should
    /// only be performed for chains with some configs stored in files.
    pub fn get_config(&self, protocol_version: ProtocolVersion) -> &Arc<EpochConfig> {
        self.store
            .range((Bound::Unbounded, Bound::Included(protocol_version)))
            .next_back()
            .unwrap_or_else(|| {
                panic!("Failed to find EpochConfig for protocol version {}", protocol_version)
            })
            .1
    }

    fn dump_epoch_config(directory: &str, version: &ProtocolVersion, config: &Arc<EpochConfig>) {
        let content = serde_json::to_string_pretty(config.as_ref()).unwrap();
        let path = PathBuf::from(directory).join(format!("{}.json", version));
        fs::write(path, content).unwrap();
    }

    /// Dumps all the configs between the beginning and end protocol versions to the given directory.
    /// If the beginning version doesn't exist, the closest config to it will be dumped.
    pub fn dump_epoch_configs_between(
        &self,
        first_version: &ProtocolVersion,
        last_version: &ProtocolVersion,
        directory: &str,
    ) {
        // Dump all the configs between the beginning and end versions, inclusive.
        self.store
            .iter()
            .filter(|(version, _)| *version >= first_version && *version <= last_version)
            .for_each(|(version, config)| {
                Self::dump_epoch_config(directory, version, config);
            });
        // Dump the closest config to the beginning version if it doesn't exist.
        if !self.store.contains_key(&first_version) {
            let config = self.get_config(*first_version);
            Self::dump_epoch_config(directory, first_version, config);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use near_primitives_core::types::ProtocolVersion;
    use near_primitives_core::version::PROTOCOL_VERSION;

    use crate::epoch_manager::{AllEpochConfig, EpochConfig};

    use super::EpochConfigStore;

    /// Checks that stored epoch config for latest protocol version matches the
    /// one generated by overrides from genesis config.
    /// If the test fails, it is either a configuration bug or the stored
    /// epoch config must be updated.
    fn test_epoch_config_store(chain_id: &str, genesis_protocol_version: ProtocolVersion) {
        let genesis_epoch_config = parse_config_file(chain_id, genesis_protocol_version).unwrap();
        let all_epoch_config = AllEpochConfig::new_with_test_overrides(
            true,
            genesis_protocol_version,
            genesis_epoch_config,
            chain_id,
            None,
        );

        let config_store = EpochConfigStore::for_chain_id(chain_id, None).unwrap();
        for protocol_version in genesis_protocol_version..=PROTOCOL_VERSION {
            let stored_config = config_store.get_config(protocol_version);
            let expected_config = all_epoch_config.generate_epoch_config(protocol_version);
            assert_eq!(
                *stored_config.as_ref(),
                expected_config,
                "Mismatch for protocol version {protocol_version}"
            );
        }
    }

    #[test]
    fn test_epoch_config_store_mainnet() {
        test_epoch_config_store("mainnet", 29);
    }

    #[test]
    fn test_epoch_config_store_testnet() {
        test_epoch_config_store("testnet", 29);
    }

    #[allow(unused)]
    fn generate_epoch_configs(chain_id: &str, genesis_protocol_version: ProtocolVersion) {
        let genesis_epoch_config = parse_config_file(chain_id, genesis_protocol_version).unwrap();
        let all_epoch_config = AllEpochConfig::new_with_test_overrides(
            true,
            genesis_protocol_version,
            genesis_epoch_config.clone(),
            chain_id,
            None,
        );

        let mut prev_config = genesis_epoch_config;
        for protocol_version in genesis_protocol_version + 1..=PROTOCOL_VERSION {
            let next_config = all_epoch_config.generate_epoch_config(protocol_version);
            if next_config != prev_config {
                tracing::info!("Writing config for protocol version {}", protocol_version);
                dump_config_file(&next_config, chain_id, protocol_version);
            }
            prev_config = next_config;
        }
    }

    #[test]
    fn test_dump_epoch_configs_mainnet() {
        let tmp_dir = tempfile::tempdir().unwrap();
        EpochConfigStore::for_chain_id("mainnet", None).unwrap().dump_epoch_configs_between(
            &55,
            &68,
            tmp_dir.path().to_str().unwrap(),
        );

        // Check if tmp dir contains the dumped files. 55, 64, 65.
        let dumped_files = fs::read_dir(tmp_dir.path()).unwrap();
        let dumped_files: Vec<_> =
            dumped_files.map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();

        assert!(dumped_files.contains(&String::from("55.json")));
        assert!(dumped_files.contains(&String::from("64.json")));
        assert!(dumped_files.contains(&String::from("65.json")));

        // Check if 55.json is equal to 48.json from res/epcoh_configs/mainnet.
        let contents_55 = fs::read_to_string(tmp_dir.path().join("55.json")).unwrap();
        let epoch_config_55: EpochConfig = serde_json::from_str(&contents_55).unwrap();
        let epoch_config_48 = parse_config_file("mainnet", 48).unwrap();
        assert_eq!(epoch_config_55, epoch_config_48);
    }

    #[test]
    #[ignore]
    fn generate_epoch_configs_mainnet() {
        generate_epoch_configs("mainnet", 29);
    }

    #[test]
    #[ignore]
    fn generate_epoch_configs_testnet() {
        generate_epoch_configs("testnet", 29);
    }

    #[allow(unused)]
    fn parse_config_file(chain_id: &str, protocol_version: ProtocolVersion) -> Option<EpochConfig> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("res/epoch_configs")
            .join(chain_id)
            .join(format!("{}.json", protocol_version));
        if path.exists() {
            let content = fs::read_to_string(path).unwrap();
            let config: EpochConfig = serde_json::from_str(&content).unwrap();
            Some(config)
        } else {
            None
        }
    }

    #[allow(unused)]
    fn dump_config_file(config: &EpochConfig, chain_id: &str, protocol_version: ProtocolVersion) {
        let content = serde_json::to_string_pretty(config).unwrap();
        fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("res/epoch_configs")
                .join(chain_id)
                .join(format!("{}.json", protocol_version)),
            content,
        )
        .unwrap()
    }
}
