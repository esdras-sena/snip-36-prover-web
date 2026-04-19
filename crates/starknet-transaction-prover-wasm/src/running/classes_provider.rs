use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use blockifier::execution::contract_class::{
    program_hints_to_casm_hints,
    CompiledClassV1,
    RunnableCompiledClass,
};
use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
use cairo_lang_utils::bigint::BigUintAsHex;
use cairo_vm::types::relocatable::MaybeRelocatable;
use starknet_api::core::{ClassHash, CompiledClassHash};
use starknet_types_core::felt::Felt;
use tracing::error;

use crate::errors::ClassesProviderError;
use crate::rpc_compat::RpcStateReader;

pub(crate) fn compiled_class_v1_to_casm(
    class: &CompiledClassV1,
) -> Result<CasmContractClass, ClassesProviderError> {
    let prime = Felt::prime();

    let bytecode: Vec<BigUintAsHex> = class
        .program
        .iter_data()
        .map(|maybe_relocatable| match maybe_relocatable {
            MaybeRelocatable::Int(felt) => Ok(BigUintAsHex { value: felt.to_biguint() }),
            MaybeRelocatable::RelocatableValue(relocatable) => {
                error!(
                    "Unexpected error: bytecode of a class contained a relocatable value: {:?}",
                    relocatable
                );
                Err(ClassesProviderError::InvalidBytecodeElement)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CasmContractClass {
        prime,
        compiler_version: String::new(),
        bytecode,
        bytecode_segment_lengths: Some(class.bytecode_segment_felt_sizes().into()),
        hints: program_hints_to_casm_hints(&class.program.shared_program_data.hints_collection)?,
        pythonic_hints: None,
        entry_points_by_type: (&class.entry_points_by_type).into(),
    })
}

fn casm_compiled_class_hash(casm: &CasmContractClass) -> CompiledClassHash {
    let big = casm.compiled_class_hash();
    let bytes = big.to_bytes_be();
    let mut padded = [0u8; 32];
    let offset = 32usize.saturating_sub(bytes.len());
    padded[offset..].copy_from_slice(&bytes[bytes.len().saturating_sub(32)..]);
    CompiledClassHash(Felt::from_bytes_be(&padded))
}

pub(crate) struct ClassesInput {
    pub(crate) compiled_classes: BTreeMap<CompiledClassHash, CasmContractClass>,
    pub(crate) class_hash_to_compiled_class_hash: HashMap<ClassHash, CompiledClassHash>,
}

#[async_trait(?Send)]
pub(crate) trait ClassesProvider {
    async fn get_classes(
        &self,
        executed_class_hashes: &HashSet<ClassHash>,
    ) -> Result<ClassesInput, ClassesProviderError>;
}

#[derive(Clone)]
pub(crate) struct WasmClassesProvider {
    pub(crate) rpc_state_reader: RpcStateReader,
}

impl WasmClassesProvider {
    pub(crate) fn new(rpc_state_reader: RpcStateReader) -> Self {
        Self { rpc_state_reader }
    }
}

#[async_trait(?Send)]
impl ClassesProvider for WasmClassesProvider {
    async fn get_classes(
        &self,
        executed_class_hashes: &HashSet<ClassHash>,
    ) -> Result<ClassesInput, ClassesProviderError> {
        let mut compiled_classes = BTreeMap::new();
        let mut class_hash_to_compiled_class_hash = HashMap::new();

        for &class_hash in executed_class_hashes {
            let runnable_compiled_class = self
                .rpc_state_reader
                .get_runnable_compiled_class_async(class_hash)
                .await
                .map_err(|e| ClassesProviderError::GetClassesError(e.to_string()))?;

            let casm = match runnable_compiled_class {
                RunnableCompiledClass::V0(_) => {
                    return Err(ClassesProviderError::DeprecatedContractError(class_hash));
                }
                RunnableCompiledClass::V1(compiled_class_v1) => {
                    compiled_class_v1_to_casm(&compiled_class_v1)?
                }
                #[allow(unreachable_patterns)]
                _ => unreachable!(),
            };

            let compiled_class_hash = casm_compiled_class_hash(&casm);
            compiled_classes.insert(compiled_class_hash, casm);
            class_hash_to_compiled_class_hash.insert(class_hash, compiled_class_hash);
        }

        Ok(ClassesInput { compiled_classes, class_hash_to_compiled_class_hash })
    }
}
