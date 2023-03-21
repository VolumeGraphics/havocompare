pub const INDEX_FILENAME: &str = "index.html";
pub const DETAIL_FILENAME: &str = "detail.html";
pub const INDEX_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Report</title>
     <style>

        .error {
            background-color: #fbcccc !important;
        }

        h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		.container {
			padding:10px;
		}

		.dataTables_wrapper {
			font-family: monospace;
    		font-size: 10pt;
		}

		table.dataTable tbody td {
			padding:0px 0px !important;
		}
		
		.text-error {
			color:red;
		}

    </style>
    <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
</head>
<body>

<div id="accordion">
{% for rule_report in rule_results %}
	<h3>
		{{ rule_report.rule.name }}
	</h3>
	<div class="container">
	<table class="report cell-border">
		<thead>
		<tr>
			<th>File</th>
			{% if rule_report.rule.FileProperties %}
				<th>File Size</th>
				<th>Creation date</th>
			{% endif %}
			<th>Result</th>
		</tr>
		</thead>
		<tbody>
			{% for file in rule_report.compare_results %}
				<tr {% if file.is_error %} class="error" {% endif %}>
					<td {% if rule_report.rule.FileProperties and file.additional_columns.0 and file.additional_columns.0.is_error %} class="text-error" {% endif %}>
						{% if file.detail_path %}
							<a href="./{{ rule_report.rule.name }}/{{ file.detail_path.path_name }}/{{ detail_filename }}">{{ file.compared_file_name }}</a>
						{% else %}
							{{ file.compared_file_name }}
						{% endif %}
					</td>
					{% if rule_report.rule.FileProperties %}
						<td {% if file.additional_columns.1.is_error %} class="text-error" {% endif %}>
							{{ file.additional_columns.1.actual_value }} / {{ file.additional_columns.1.nominal_value }}
						</td>
						<td {% if file.additional_columns.2.is_error %} class="text-error" {% endif %}>
							{{ file.additional_columns.2.actual_value }} / {{ file.additional_columns.2.nominal_value }}
						</td>
					{% endif %}
					<td>{% if file.is_error %} <span class="text-error">&#10006;</span> {% else %} <span style="color:green;">&#10004;</span> {% endif %}</td>
				</tr>
			{% endfor %}
		</tbody>
	</table>
	</div>
{% endfor %}
</div>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script
  src="https://code.jquery.com/ui/1.13.2/jquery-ui.min.js"
  integrity="sha256-lSjKY0/srUM9BE3dPm+c4fBo1dky2v27Gdjm2uoZaL0="
  crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('.report');
    });
    
    $(function() {
        $( "#accordion" ).accordion();
    });
</script>
</body>
</html>
"###;

pub const PLAIN_TEXT_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"###;

pub const PLAIN_IMAGE_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

        table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
            <tr>
                <td>{{ error }}</td>
            </tr>
    </tbody>
</table>

<p>
<h3>Nominal:</h3>
<img src="./{{ nominal_image }}" />
</p>

<p>
<h3>Actual:</h3>
<img src="./{{ actual_image }}" />
</p>

<p>
<h3>Diff:</h3>
<img src="./{{ diff_image }}" />
</p>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"###;

pub const PLAIN_CSV_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		.actual {
			color: #0d6efdf0;
		}
		
		.diffs {
			color: #FF4646;
		}
		
		table.dataTable {
			border: 1px solid #999999;
		}

		.dataTables_wrapper {
			font-family: monospace;
    		font-size: 10pt;
		}
		
		table.dataTable th:not(:last-child), table.dataTable td:not(:last-child) {
			border-right: 1px solid #999999;
		}

		.error {
            background-color: #fbcccc !important;
        }

        table.dataTable td, table.dataTable th {
			white-space:nowrap;
		}

		table.dataTable tbody td, table.dataTable thead th {
			padding:0px 0px;
		}

    </style>
</head>
<body>

<h3>Compare Result</h3>
<p>
	<table>
		<tbody>
			<tr>
				<td>Left file (nominal):</td>
				<td>{{ nominal }}</td>
			</tr>
				<tr>
				<td>Right file (actual):</td>
				<td>{{ actual }}</td>
			</tr>
		</tbody>
	</table>
</p>

{% if headers.columns|length <= 0 %}
	<p><i>Header preprocessing not enabled in config</i></p>
{% endif %}
<table id="report" class="cell-border">
    <thead>
    {% if headers.columns|length > 0 %}
    	<tr>
	    	<th>{% if headers.has_diff %}<br>&nbsp;{% endif %}</th>
	    	<th>{% if headers.has_diff %}&nbsp;<br>{% endif %}</th>
			{% for col in headers.columns %}
				<th>
					{{ col.nominal_value }}
					{% if headers.has_diff %}
						<div class="{% if col.nominal_value != col.actual_value %} actual {% endif %}">{{ col.actual_value }}</div>
					{% endif %}
				</th>
			{% endfor %}
		</tr>
    {% else %}
	    <tr>
	    	<th>&nbsp;</th>
	    	<th>&nbsp;</th>
	    	{% for col in rows[0].columns %}
		    	<th>&nbsp;</th>
	    	{% endfor %}
	    </tr>
    {% endif %}
    </thead>
    <tbody>
        {% for row in rows %}
            <tr {% if row.has_error %} class="error" {% endif %}>
            	<td data-order="{{ loop.index0 }}">{{ loop.index0 }}{% if row.has_diff or row.has_error %}<br>&nbsp;{% endif %}</td>
            	<td data-order="{{ loop.index0 }}">{% if row.has_diff or row.has_error %}&nbsp;<br>{% endif %}{{ loop.index0 }}</td>
            	{% for col in row.columns %}
					<td>
					{{ col.nominal_value }}
					{% if row.has_diff or row.has_error %}
						<div class="{% if col.diffs|length > 0 %} diffs {% elif col.nominal_value != col.actual_value %} actual {% else %} {% endif %}">
						{{ col.actual_value }}
						</div>
					{% endif %}
                	 </td>
                {% endfor %}
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report', {
			lengthMenu: [ [5, 10, 25, 50, -1], [5, 10, 25, 50, "All"] ],
			iDisplayLength: -1,
			columnDefs: [
    			{ type: "num", "targets": 0 },
    			{ type: "num", "targets": 1 }
			],
			bPaginate: false,
    		bLengthChange: false,
		});
    });
</script>

</body>
</html>
"###;

pub const PLAIN_PDF_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>

     <style>

		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

        .helper {
        	color:orange;
        	font-weight:bold;
        }

		.helper a {
			color:orange;
		}

		.has_diff {
			color: #0d6efdf0;
		}

		.has_error {
			color:red;
		}

		#compare th {
			text-align:left;
			background-color: #cccccc;
			padding:10px;
		}

		#compare td:first-child {
			border-right: 1px solid black;
		}

		table#compare {
			border:1px solid grey;
		}

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<div class="helper">
This is for viewing only.
The extracted exact text can be downloaded here: <a href="./{{ nominal_extracted_filename }}">nominal</a> and <a href="./{{ actual_extracted_filename }}">actual</a>
</div>
<table id="compare">
	<thead>
		<tr>
			<th></th>
			<th>Nominal</th>
			<th>Actual</th>
		</tr>
	</thead>
	<tbody>
	{% for line in combined_lines %}
		<tr>
			<td>{{ loop.index }}</td>
			<td>{{ line.nominal_value|safe }}</td>
			<td>
				{% if line.diffs|length > 0 %}
					<span class="has_error">{{ line.actual_value|safe }}</span>
				{% elif line.actual_value != line.nominal_value %}
					<span class="has_diff">{{ line.actual_value|safe }}</span>
				{% else %}
					{{ line.actual_value|safe }}
				{% endif %}
			</td>
		</tr>
	{% endfor %}
	</tbody>
</table>

<br>
<br>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"###;

pub const ERROR_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error</title>
    <style>


    </style>
</head>
<body>

<p>
	<table>
		<tbody>
			<tr>
				<td>Nominal:</td>
				<td>{{ nominal }}</td>
			</tr>
				<tr>
				<td>Actual:</td>
				<td>{{ actual }}</td>
			</tr>
		</tbody>
	</table>
</p>

<p style="color:red">
	{{ error }}
</p>


</body>
</html>
"###;
