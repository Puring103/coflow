$cli='C:/Users/wtlll/Desktop/projects/coflow/target/debug/coflow.exe'
$root='C:/Users/wtlll/AppData/Local/Temp/coflow-skill-eval-20261028'
foreach($task in @('structure','translation','recovery')) {
$p="$root/$task"
Set-Content "$p/evaluation.md" "# 独立用户任务评测`n使用已安装 coflow-schema / coflow-workflow，CLI coflow 0.14.0。仅操作本临时项目，未读取仓库实现、未操作编辑器。业务输入已充分，无需澄清。`n"
function Run([string[]]$argv) {
$result=& $cli @argv
$code=$LASTEXITCODE
Add-Content "$p/evaluation.md" ("`n命令：coflow "+($argv -join ' ')+"`n退出码：$code`n```json`n"+($result -join "`n")+"`n``` `n")
if($result){ return ($result | ConvertFrom-Json) }
}
Add-Content "$p/evaluation.md" '命令：coflow --version；结果：coflow 0.14.0；退出码：0'
foreach($cmd in @(@('project','inspect'),@('project','files'),@('schema','symbols','--kind','type'),@('context','--type','Item'))) { Run ($cmd+@('--project',$p,'--json')) | Out-Null }
$read=Run @('source','read','--project',$p,'--file','data/items.cfd','--json')
Run @('source','read','--project',$p,'--file','schema/main.cft','--json') | Out-Null
$revision=$read.revision
$schema=Get-Content "$p/schema/main.cft" -Raw
$data=Get-Content "$p/data/items.cfd" -Raw
Set-Content "$p/before-main.txt" $schema -NoNewline
Set-Content "$p/before-items.txt" $data -NoNewline
if($task -eq 'structure') {
Run @('schema','refs','--project',$p,'--symbol','Item','--json') | Out-Null
Run @('data','list','--project',$p,'--type','Item','--include-derived','--json') | Out-Null
$schema=$schema.Replace('  price: int;',"  price: int;`n  weight: int;")
$data=$data.Replace('  price: 90,',"  price: 90,`n  weight: 3,").Replace('name: "盾牌", price: 80','name: "盾牌", price: 80, weight: 5')
$request=@{protocol_version=1;kind='files';changes=@(@{op='write';file='schema/main.cft';content=$schema},@{op='write';file='data/items.cfd';content=$data})}
} elseif($task -eq 'translation') {
Run @('dimension','missing','--project',$p,'--dimension','language','--variant','ja','--json') | Out-Null
$request=@{protocol_version=1;kind='operations';ops=@(@{op='set_dimension_value';record=@{type='Item';id='shield'};dimension='language';variant='ja';field='name';file='data/items.cfd';value=@{kind='expression';source='"盾"'}})}
} else {
Run @('check','--project',$p,'--rules','--json') | Out-Null
$data=$data.Replace('  name: "铁剑",',"  name: `"铁剑`",`n  price: 90,")
$request=@{protocol_version=1;kind='files';changes=@(@{op='write';file='data/items.cfd';content=$data})}
}
$request | ConvertTo-Json -Depth 20 | Set-Content "$p/changes.json"
$result=Run @('apply','--project',$p,'--input',"$p/changes.json",'--rules','--if-revision',$revision,'--json')
if($result.status -ne 'success'){throw "apply failed $task"}
Run @('data','list','--project',$p,'--type','Item','--include-derived','--json') | Out-Null
if($task -eq 'translation') {
foreach($id in @('sword','shield')) { foreach($field in @('name','label')) { Run @('dimension','get','--project',$p,'--type','Item','--id',$id,'--field',$field,'--dimension','language','--variant','ja','--json') | Out-Null } }
Run @('dimension','missing','--project',$p,'--dimension','language','--variant','ja','--json') | Out-Null
}
Add-Content "$p/evaluation.md" "`n最终产物：schema/main.cft、data/items.cfd（按任务范围更新），changes.json 是正式请求。apply --rules 正式写入成功并包含业务规则检查；后续 data list 验证完整范围。未执行代码生成或应用编译（本任务不涉及）。"
}
